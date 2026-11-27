use toml::Value as TomlValue;
use xdcode_app_server::in_process::EmbeddedNetworkPolicy;
use xdcode_cloud_config::chatgpt_config_bundle_loader_for_storage;
use xdcode_config::CloudConfigBundleLoader;
use xdcode_config::ConfigLoadOptions;
use xdcode_core::config::bootstrap_auth_config;
use xdcode_core::config::load_config_toml_with_layer_stack;
use xdcode_utils_absolute_path::AbsolutePathBuf;

use super::DebugSandboxConfigOptions;
use super::ManagedRequirementsMode;

pub(super) async fn bootstrap_cloud_config_bundle(
    cli_overrides: &[(String, TomlValue)],
    options: &DebugSandboxConfigOptions,
    resolve_xdcode_home: impl FnOnce() -> std::io::Result<AbsolutePathBuf>,
    strict_config: bool,
) -> anyhow::Result<CloudConfigBundleLoader> {
    if options.sandbox_state.sandbox_state_json.is_some()
        || !matches!(
            options.managed_requirements_mode,
            ManagedRequirementsMode::Include
        )
    {
        return Ok(CloudConfigBundleLoader::default());
    }

    let xdcode_home = resolve_xdcode_home()?;
    let cwd = match options.cwd.as_deref() {
        Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd)?,
        None => AbsolutePathBuf::current_dir()?,
    };
    let network_policy = EmbeddedNetworkPolicy::load(&options.loader_overrides).await;
    let bootstrap_config = load_config_toml_with_layer_stack(
        xdcode_home.as_path(),
        Some(&cwd),
        cli_overrides.to_vec(),
        ConfigLoadOptions {
            loader_overrides: options.loader_overrides.clone(),
            strict_config,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await?;
    Ok(chatgpt_config_bundle_loader_for_storage(
        network_policy.bind_bootstrap_auth(bootstrap_auth_config(
            xdcode_home.as_path(),
            &bootstrap_config,
        )?),
        /*enable_xdcode_api_key_env*/ false,
    )
    .await?)
}

#[cfg(test)]
#[path = "cloud_config_tests.rs"]
mod tests;
