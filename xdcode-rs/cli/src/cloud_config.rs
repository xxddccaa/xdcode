use anyhow::Context;
use anyhow::Result;
use xdcode_app_server::in_process::EmbeddedNetworkPolicy;
use xdcode_cloud_config::cloud_config_bundle_loader_for_storage;
use xdcode_config::CloudConfigBundleLoader;
use xdcode_config::ConfigLoadOptions;
use xdcode_core::config::Config;
use xdcode_core::config::ConfigBuilder;
use xdcode_core::config::ConfigOverrides;
use xdcode_core::config::LoaderOverrides;
use xdcode_core::config::bootstrap_auth_config;
use xdcode_core::config::find_xdcode_home;
use xdcode_core::config::load_config_toml_with_layer_stack;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_cli::CliConfigOverrides;

pub(crate) async fn load_config(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<Config> {
    let mut config = config_builder(
        config_overrides,
        loader_overrides,
        ConfigOverrides::default(),
    )
    .await?
    .build()
    .await
    .context("failed to load configuration")?;
    EmbeddedNetworkPolicy::default().activate(&mut config);
    Ok(config)
}

pub(crate) async fn config_builder(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
    harness_overrides: ConfigOverrides,
) -> Result<ConfigBuilder> {
    let network_policy = EmbeddedNetworkPolicy::load(&loader_overrides).await;
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let xdcode_home = find_xdcode_home().context("failed to resolve XDCODE_HOME")?;
    let cwd = match harness_overrides.cwd.as_deref() {
        Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd),
        None => AbsolutePathBuf::current_dir(),
    }
    .context("failed to resolve current directory")?;
    let bootstrap_config = load_config_toml_with_layer_stack(
        xdcode_home.as_path(),
        Some(&cwd),
        cli_overrides.clone(),
        ConfigLoadOptions {
            loader_overrides: loader_overrides.clone(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await
    .context("failed to load bootstrap configuration")?;
    let cloud_config_bundle = cloud_config_bundle_loader_for_storage(
        network_policy.bind_bootstrap_auth(
            bootstrap_auth_config(xdcode_home.as_path(), &bootstrap_config)
                .context("failed to resolve cloud configuration authentication")?,
        ),
        /*enable_xdcode_api_key_env*/ false,
    )
    .await
    .context("failed to initialize cloud configuration authentication")?;

    Ok(ConfigBuilder::default()
        .xdcode_home(xdcode_home.to_path_buf())
        .cli_overrides(cli_overrides)
        .loader_overrides(loader_overrides)
        .harness_overrides(harness_overrides)
        .cloud_config_bundle(cloud_config_bundle)
        .fallback_cwd(Some(cwd.to_path_buf())))
}
