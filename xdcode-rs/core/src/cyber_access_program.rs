//! Forward explicit programs while leaving entitlement and model policy to the server.

use std::sync::Arc;
use xdcode_api::AccessPrograms;
use xdcode_features::Feature;
use xdcode_login::XdcodeAuth;
use xdcode_protocol::error::Result;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::turn_input::CyberAccessProgram;

/// Host requirement for every model request issued by this thread.
///
/// Install in thread extension data before startup and reapply on cold resume.
/// Core snapshots it before extension callbacks; it is not inherited or persisted.
/// Every request must resolve to one listed program; backend authorization remains authoritative.
#[derive(Clone, Debug)]
pub struct RequiredCyberAccessPrograms {
    pub allowed: Vec<CyberAccessProgram>,
}

pub(crate) struct RequiredCyberAccessProgramsSnapshot(pub Arc<RequiredCyberAccessPrograms>);

impl RequiredCyberAccessPrograms {
    /// Read Core's startup snapshot, independent of later edits to the public attachment.
    pub fn for_thread(data: &xdcode_extension_api::ExtensionData) -> Option<Arc<Self>> {
        data.get::<RequiredCyberAccessProgramsSnapshot>()
            .map(|snapshot| Arc::clone(&snapshot.0))
    }
}

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
        if !config
            .model_provider
            .supports_cyber_access_programs(&config.model_provider_id)
        {
            Self::UnsupportedProvider
        } else if config.features.enabled(Feature::ApiKeyCyberAccessPrograms) {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

pub(crate) fn for_provider(
    config: &crate::config::Config,
    program: Option<CyberAccessProgram>,
) -> Option<CyberAccessProgram> {
    program.filter(|_| {
        config
            .model_provider
            .supports_cyber_access_programs(&config.model_provider_id)
    })
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
    if auth.is_chatgpt_auth() || matches!(auth, XdcodeAuth::Headers(_)) {
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
