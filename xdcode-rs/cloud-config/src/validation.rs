use xdcode_config::AbsolutePathBuf;
use xdcode_config::CloudConfigBundle;
use xdcode_config::CloudConfigBundleLayers;
use xdcode_config::CloudConfigBundleLoadError;
use xdcode_config::CloudConfigBundleLoadErrorCode;
use xdcode_config::compose_requirements;
use xdcode_config::config_toml::validate_model_providers;

pub(crate) fn validate_bundle(
    bundle: &CloudConfigBundle,
    base_dir: &AbsolutePathBuf,
) -> Result<(), CloudConfigBundleLoadError> {
    let bundle_layers =
        CloudConfigBundleLayers::from_bundle(bundle.clone(), base_dir).map_err(|_| {
            CloudConfigBundleLoadError::new(
                CloudConfigBundleLoadErrorCode::InvalidBundle,
                /*status_code*/ None,
                "invalid cloud config bundle: invalid TOML",
            )
        })?;
    let CloudConfigBundleLayers {
        enterprise_managed_config: _,
        enterprise_managed_requirements,
    } = bundle_layers;

    let requirements = compose_requirements(enterprise_managed_requirements).map_err(|_| {
        CloudConfigBundleLoadError::new(
            CloudConfigBundleLoadErrorCode::InvalidBundle,
            /*status_code*/ None,
            "invalid cloud config bundle: invalid requirements",
        )
    })?;
    if let Some(providers) = requirements.and_then(|requirements| requirements.model_providers) {
        validate_model_providers(&providers).map_err(|_| {
            CloudConfigBundleLoadError::new(
                CloudConfigBundleLoadErrorCode::InvalidBundle,
                /*status_code*/ None,
                "invalid cloud config bundle: invalid model provider configuration",
            )
        })?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
