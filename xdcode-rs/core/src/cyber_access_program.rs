//! Forward explicit programs while leaving entitlement and model policy to the server.

use xdcode_api::AccessPrograms;
use xdcode_features::Feature;
use xdcode_login::XdcodeAuth;
use xdcode_model_provider_info::PROVIDER_PROVIDER_ID;
use xdcode_protocol::error::Result;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::turn_input::CyberAccessProgram;

/// Configured support for explicit Cyber programs on API-key requests.
#[derive(Clone, Copy, Debug)]
pub enum ApiKeyCyberAccessPrograms {
    UnsupportedProvider,
    Disabled,
    Enabled,
}

impl ApiKeyCyberAccessPrograms {
    /// Captures the same provider and feature policy used by ordinary model requests.
    pub fn from_config(config: &crate::config::Config) -> Self {
        if config.model_provider_id != PROVIDER_PROVIDER_ID {
            Self::UnsupportedProvider
        } else if config.features.enabled(Feature::ApiKeyCyberAccessPrograms) {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

pub(crate) fn for_provider(
    provider_id: &str,
    program: Option<CyberAccessProgram>,
) -> Option<CyberAccessProgram> {
    program.filter(|_| provider_id == PROVIDER_PROVIDER_ID)
}

/// Resolves an explicit program using the request's current authentication.
pub fn for_auth(
    auth: Option<&XdcodeAuth>,
    program: Option<CyberAccessProgram>,
    policy: ApiKeyCyberAccessPrograms,
) -> Result<Option<AccessPrograms>> {
    let Some(program) = program else {
        return Ok(None);
    };
    let Some(auth) = auth else {
        return Ok(None);
    };
    if auth.is_chatgpt_auth() {
        return Ok(Some(program.into()));
    }
    if !auth.is_api_key_auth() {
        return Ok(None);
    }
    match policy {
        ApiKeyCyberAccessPrograms::UnsupportedProvider => Ok(None),
        ApiKeyCyberAccessPrograms::Disabled => Err(XdcodeErr::InvalidRequest(
            "Cyber access programs are disabled for this API-key session.".to_owned(),
        )),
        ApiKeyCyberAccessPrograms::Enabled => Ok(Some(program.into())),
    }
}
