//! Command-line startup for `xdcode exec-server`.
//!
//! Transport, configuration, authentication, and shutdown are kept together.

use clap::Parser;
use xdcode_arg0::Arg0DispatchPaths;
use xdcode_cloud_config::cloud_config_bundle_loader_for_storage;
use xdcode_config::LoaderOverrides;
use xdcode_core::config::Config;
use xdcode_core::config::ConfigBuilder;
use xdcode_core::config::ConfigLoadOptions;
use xdcode_core::config::bootstrap_auth_config;
use xdcode_core::config::find_xdcode_home;
use xdcode_core::config::load_config_toml_with_layer_stack;
use xdcode_exec_server::ExecServerRuntimeOptions;
use xdcode_http_client::HttpClientFactory;
use xdcode_http_client::OutboundProxyPolicy;
use xdcode_login::is_workload_identity_selected;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_cli::CliConfigOverrides;
use xdcode_websocket_auth::WebsocketAuthArgs;

use crate::exec_server_telemetry;

/// [EXPERIMENTAL] Run the standalone exec-server service.
#[derive(Debug, Parser)]
pub(super) struct ExecServerCommand {
    /// Error out when config.toml contains fields that are not recognized by this version of Xdcode.
    #[arg(
        id = "exec_server_strict_config",
        long = "strict-config",
        default_value_t = false,
        global = true
    )]
    pub(super) strict_config: bool,

    /// Linux PID namespace: isolate (default) or inherit. Inherit allows signals to
    /// other same-UID processes; enable only when provisioning a dedicated environment.
    #[arg(long, value_name = "MODE", default_value = "isolate", global = true)]
    linux_sandbox_pid_namespace: xdcode_sandboxing::LinuxSandboxPidNamespace,

    /// Allow permitted private IP destinations to use the configured upstream proxy.
    /// If no valid upstream proxy applies to the request protocol, connect directly.
    /// Loopback stays local. This flag does not require upstream routing.
    #[arg(
        long,
        env = "XDCODE_EXEC_SERVER_PROXY_PRIVATE_IPS_VIA_UPSTREAM",
        global = true
    )]
    proxy_private_ips_via_upstream: bool,

    /// Maximum number of requests to process concurrently on each connection.
    #[arg(
        long = "concurrent-requests",
        value_name = "COUNT",
        default_value = "1"
    )]
    request_dispatch_mode: xdcode_exec_server::RequestDispatchMode,

    /// Transport endpoint URL. Supported values: `ws://IP:PORT` (default), `stdio`, `stdio://`.
    #[arg(long = "listen", value_name = "URL")]
    listen: Option<String>,

    #[command(flatten)]
    websocket_auth: WebsocketAuthArgs,
}

impl ExecServerCommand {
    /// Starts the executor with the given runtime paths and configuration overrides.
    pub(super) async fn run(
        self,
        arg0_paths: &Arg0DispatchPaths,
        root_config_overrides: &CliConfigOverrides,
    ) -> anyhow::Result<()> {
        let strict_config = self.strict_config;
        let websocket_auth = self.websocket_auth.try_into_settings()?;
        let xdcode_self_exe = arg0_paths
            .xdcode_self_exe
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Xdcode executable path is not configured"))?;
        let mut runtime_paths = ExecServerRuntimeOptions::new(
            xdcode_self_exe,
            arg0_paths.xdcode_linux_sandbox_exe.clone(),
        )?
        .with_linux_sandbox_pid_namespace(self.linux_sandbox_pid_namespace)
        .with_proxy_private_ips_via_upstream(self.proxy_private_ips_via_upstream);
        // Config loading below owns error handling (non-strict stdio can proceed
        // without config). Never retain other startup flags in the executor.
        if let Ok(overrides) = root_config_overrides.parse_overrides() {
            let cli_flags = xdcode_config::build_cli_overrides_layer(&overrides);
            runtime_paths.prefer_mxc = cli_flags
                .get("features")
                .and_then(|features| features.get("prefer_mxc"))
                .and_then(toml::Value::as_bool);
        }
        {
            let config_result = load_exec_server_config(
                root_config_overrides,
                strict_config,
                /*enable_workload_identity*/ false,
            )
            .await;
            let config = if strict_config {
                Some(config_result?)
            } else {
                config_result.ok()
            };
            let (_otel, telemetry) = exec_server_telemetry::init(config.as_ref());
            #[cfg(target_os = "macos")]
            let runtime_paths = runtime_paths.with_allowed_symlinked_xdcode_home(
                config.as_ref().and_then(|config| {
                    xdcode_config::allowed_symlinked_xdcode_home(
                        &config.config_layer_stack,
                        &config.xdcode_home,
                    )
                }),
            );
            let http_client_factory = config
                .as_ref()
                .map(Config::http_client_factory)
                .unwrap_or_else(|| HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault));
            let listen_url = self
                .listen
                .unwrap_or_else(|| xdcode_exec_server::DEFAULT_LISTEN_URL.to_string());
            let run = exec_server_telemetry::run_until_shutdown(
                xdcode_exec_server::run_main_with_telemetry(
                    &listen_url,
                    runtime_paths,
                    telemetry,
                    http_client_factory,
                    self.request_dispatch_mode,
                    websocket_auth,
                ),
                exec_server_telemetry::ParentLifetime::Independent,
                exec_server_telemetry::ShutdownBehavior::Immediate,
            );
            run.await.map_err(anyhow::Error::from_boxed)
        }
    }
}

async fn load_exec_server_config(
    root_config_overrides: &CliConfigOverrides,
    strict_config: bool,
    enable_workload_identity: bool,
) -> anyhow::Result<xdcode_core::config::Config> {
    let cli_kv_overrides = root_config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let bootstrap_cli_overrides = cli_kv_overrides.clone();
    let mut builder = ConfigBuilder::default()
        .cli_overrides(cli_kv_overrides)
        .strict_config(strict_config);
    if enable_workload_identity && is_workload_identity_selected() {
        let xdcode_home = find_xdcode_home()?;
        let bootstrap_cwd = AbsolutePathBuf::current_dir()?;
        let bootstrap_config = load_config_toml_with_layer_stack(
            &xdcode_home,
            Some(&bootstrap_cwd),
            bootstrap_cli_overrides,
            ConfigLoadOptions {
                loader_overrides: LoaderOverrides::default(),
                strict_config,
                cloud_config_bundle: Default::default(),
            },
        )
        .await?;
        let bootstrap_auth_config = bootstrap_auth_config(&xdcode_home, &bootstrap_config)?;
        let cloud_config_bundle = cloud_config_bundle_loader_for_storage(
            bootstrap_auth_config,
            /*enable_xdcode_api_key_env*/ false,
        )
        .await?;
        builder = builder.cloud_config_bundle(cloud_config_bundle);
    }
    Ok(builder.build().await?)
}
