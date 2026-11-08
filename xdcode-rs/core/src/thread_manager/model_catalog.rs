use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use crate::config::Config;
use xdcode_features::Feature;
use xdcode_login::AuthManager;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_models_manager::manager::SharedModelsManager;
use xdcode_protocol::model_types::ModelsResponse;
use xdcode_protocol::protocol::SessionSource;

const CATALOG_CACHE_CAPACITY: usize = 32;

#[derive(Clone)]
pub(crate) struct ThreadModelCatalog {
    entries: Arc<Mutex<Vec<CatalogEntry>>>,
}

struct CatalogEntry {
    provider: ModelProviderInfo,
    models: Option<ModelsResponse>,
    home: PathBuf,
    api_key_discovery: bool,
    auth: Arc<AuthManager>,
    manager: SharedModelsManager,
}

impl CatalogEntry {
    fn matches(&self, config: &Config, auth: &Arc<AuthManager>) -> bool {
        Arc::ptr_eq(&self.auth, auth)
            && self.home == config.xdcode_home.as_path()
            && self.models == config.model_catalog
            && self.api_key_discovery == config.features.enabled(Feature::ApiKeyModelDiscovery)
            && self.provider
                == ModelProviderInfo {
                    request_max_retries: self.provider.request_max_retries,
                    stream_max_retries: self.provider.stream_max_retries,
                    supports_websockets: self.provider.supports_websockets,
                    ..config.model_provider.clone()
                }
    }
}

impl ThreadModelCatalog {
    pub(crate) fn new(
        config: &Config,
        auth: Arc<AuthManager>,
        manager: SharedModelsManager,
    ) -> Self {
        Self::with_settings(
            config.model_provider.clone(),
            config.model_catalog.clone(),
            config.xdcode_home.to_path_buf(),
            config.features.enabled(Feature::ApiKeyModelDiscovery),
            auth,
            manager,
        )
    }

    pub(crate) fn with_settings(
        provider: ModelProviderInfo,
        models: Option<ModelsResponse>,
        home: PathBuf,
        api_key_discovery: bool,
        auth: Arc<AuthManager>,
        manager: SharedModelsManager,
    ) -> Self {
        Self {
            entries: Arc::new(Mutex::new(vec![CatalogEntry {
                provider,
                models,
                home,
                api_key_discovery,
                auth,
                manager,
            }])),
        }
    }

    pub(crate) fn for_config(
        &self,
        config: &Config,
        auth: Arc<AuthManager>,
    ) -> SharedModelsManager {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = entries.iter().find(|entry| entry.matches(config, &auth)) {
            return Arc::clone(&entry.manager);
        }
        let manager = super::build_models_manager(config, Arc::clone(&auth));
        if entries.len() == CATALOG_CACHE_CAPACITY {
            entries.remove(0);
        }
        entries.push(CatalogEntry {
            provider: config.model_provider.clone(),
            models: config.model_catalog.clone(),
            home: config.xdcode_home.to_path_buf(),
            api_key_discovery: config.features.enabled(Feature::ApiKeyModelDiscovery),
            auth,
            manager: Arc::clone(&manager),
        });
        manager
    }

    pub(crate) async fn for_thread(
        &self,
        config: &Config,
        auth: Arc<AuthManager>,
        originator: &str,
        source: &SessionSource,
    ) -> SharedModelsManager {
        let manager = self.for_config(config, auth);
        let manager = manager.for_originator(originator).unwrap_or(manager);
        if source.is_non_root_agent() && config.model_catalog_id.is_none() {
            manager
                .ensure_initialized(config.http_client_factory())
                .await;
        }
        manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xdcode_login::XdcodeAuth;
    use xdcode_models_manager::manager::StaticModelsManager;

    async fn catalog() -> (
        Config,
        Arc<AuthManager>,
        SharedModelsManager,
        ThreadModelCatalog,
    ) {
        let config = crate::config::test_config().await;
        let auth = AuthManager::from_auth_for_testing(XdcodeAuth::from_api_key("test-catalog-key"));
        let manager: SharedModelsManager = Arc::new(StaticModelsManager::new(
            None,
            xdcode_models_manager::bundled_models_response().expect("bundled catalog"),
        ));
        let catalog = ThreadModelCatalog::new(&config, Arc::clone(&auth), Arc::clone(&manager));
        (config, auth, manager, catalog)
    }

    #[tokio::test]
    async fn inherited_catalog_retains_the_injected_manager_and_transport_tuning() {
        let (mut config, auth, manager, catalog) = catalog().await;
        config.model_provider.request_max_retries = Some(3);
        config.model_provider.stream_max_retries = Some(4);
        config.model_provider.supports_websockets = !config.model_provider.supports_websockets;
        let inherited = catalog.clone();
        assert!(Arc::ptr_eq(&inherited.for_config(&config, auth), &manager));
    }

    #[tokio::test]
    async fn provider_routes_headers_and_auth_owners_have_independent_catalogs() {
        let (config, auth, manager, catalog) = catalog().await;
        let mut changed = config.clone();
        changed.model_provider.base_url = Some("https://other.example.invalid/v1".into());
        let routed = catalog.for_config(&changed, Arc::clone(&auth));
        assert!(!Arc::ptr_eq(&routed, &manager));
        assert!(Arc::ptr_eq(
            &routed,
            &catalog.for_config(&changed, Arc::clone(&auth))
        ));
        changed.model_provider.http_headers =
            Some([("X-Catalog".into(), "separate".into())].into());
        assert!(!Arc::ptr_eq(
            &routed,
            &catalog.for_config(&changed, Arc::clone(&auth))
        ));
        let different_auth =
            AuthManager::from_auth_for_testing(XdcodeAuth::from_api_key("test-catalog-key"));
        assert!(!Arc::ptr_eq(
            &manager,
            &catalog.for_config(&config, different_auth)
        ));
        assert!(Arc::ptr_eq(&manager, &catalog.for_config(&config, auth)));
    }

    #[tokio::test]
    async fn configured_catalog_and_home_changes_do_not_reuse_old_metadata() {
        let (config, auth, manager, catalog) = catalog().await;
        let mut changed = config.clone();
        changed.model_catalog = Some(ModelsResponse { models: Vec::new() });
        assert!(!Arc::ptr_eq(
            &manager,
            &catalog.for_config(&changed, Arc::clone(&auth))
        ));
        changed = config.clone();
        changed.xdcode_home = config.xdcode_home.join("different-home");
        assert!(!Arc::ptr_eq(
            &manager,
            &catalog.for_config(&changed, Arc::clone(&auth))
        ));
        assert!(Arc::ptr_eq(&manager, &catalog.for_config(&config, auth)));
    }

    #[tokio::test]
    async fn catalog_cache_is_bounded_without_evicting_the_most_recent_route() {
        let (mut config, auth, _, catalog) = catalog().await;
        for index in 0..CATALOG_CACHE_CAPACITY + 8 {
            config.model_provider.base_url =
                Some(format!("https://route-{index}.example.invalid/v1"));
            catalog.for_config(&config, Arc::clone(&auth));
        }
        let last = catalog.for_config(&config, Arc::clone(&auth));
        assert!(Arc::ptr_eq(&last, &catalog.for_config(&config, auth)));
        assert_eq!(
            catalog.entries.lock().expect("catalog cache").len(),
            CATALOG_CACHE_CAPACITY
        );
    }
}
