//! Fixtures for integration tests that seed provider credentials and model caches.

use xdcode_login::XdcodeAuth;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_models_manager::cache::ModelsCacheEntry;
use xdcode_protocol::model_types::ModelInfo;

/// Constructs a cache fixture matching the supplied test provider and authentication.
pub fn models_cache_entry(
    provider_info: &ModelProviderInfo,
    auth: Option<&XdcodeAuth>,
    models: Vec<ModelInfo>,
) -> ModelsCacheEntry {
    ModelsCacheEntry {
        fetched_at: std::time::SystemTime::now().into(),
        etag: None,
        client_version: Some(xdcode_models_manager::client_version_to_whole()),
        identity: crate::models_identity::identity(provider_info, auth).ok(),
        models,
    }
}

pub use crate::shared_state::test_support::seed_gateway_auth;
