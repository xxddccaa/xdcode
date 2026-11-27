//! Keep each catalog bound to its provider and auth owner while preserving host cache policy.

use crate::config::Config;
use crate::runtime_auth::RuntimeAuthSlot;
use crate::runtime_auth::runtime_for_config;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use xdcode_features::Feature;
use xdcode_features::RuntimeAuthMode;
use xdcode_login::AuthManager;
use xdcode_model_provider::ModelCatalogFactory;
use xdcode_model_provider::ProviderAuthContext;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_models_manager::manager::ModelCatalogSettings;
use xdcode_models_manager::manager::SharedModelsManager;
use xdcode_protocol::model_types::ModelsResponse;
use xdcode_protocol::protocol::SessionSource;
use xdcode_runtime_agent_instance::RuntimeAuthManager;

const CATALOG_CACHE_CAPACITY: usize = 32;

/// The startup catalog and the credential/runtime owner used to construct it.
pub struct StartupModels {
    pub(super) credentials: Arc<AuthManager>,
    pub(super) runtime_auth: RuntimeAuthSlot,
    pub(super) installation_id: String,
    pub(super) catalog: ThreadModelCatalog,
}

impl StartupModels {
    /// Share the startup catalog with host discovery and prefetch callers.
    pub fn models_manager(&self) -> SharedModelsManager {
        Arc::clone(&self.catalog.manager)
    }
}

/// Construct the catalog and runtime owner together using the host's catalog factory.
/// Use `ModelCatalogSettings` for standard provider catalogs with a disk, custom, or no cache.
/// Runtime registration remains lazy; this does not register an instance or generate a key.
pub fn build_startup_models(
    config: &Config,
    credentials: Arc<AuthManager>,
    installation_id: String,
    factory: Arc<dyn ModelCatalogFactory>,
) -> StartupModels {
    let runtime_auth = RuntimeAuthSlot::default();
    let runtime = runtime_for_config(
        &runtime_auth,
        config,
        Arc::clone(&credentials),
        &installation_id,
    );
    let catalog = ThreadModelCatalog::new_with_home(
        config.model_provider.clone(),
        config.model_catalog.clone(),
        Some(config.xdcode_home.to_path_buf()),
        factory,
        config.features.enabled(Feature::ApiKeyModelDiscovery),
        ProviderAuthContext {
            credentials: Arc::clone(&credentials),
            runtime_auth: runtime.map(|runtime| (runtime, config.runtime_auth_mode)),
        },
    );
    StartupModels {
        credentials,
        runtime_auth,
        installation_id,
        catalog,
    }
}

#[derive(Clone)]
pub(crate) struct ThreadModelCatalog {
    pub(super) manager: SharedModelsManager,
    provider: ModelProviderInfo,
    catalog: Option<ModelsResponse>,
    home: Option<PathBuf>,
    factory: Arc<dyn ModelCatalogFactory>,
    api_key_model_discovery_default: bool,
    runtime_mode: Option<RuntimeAuthMode>,
    entries: Arc<Mutex<Vec<CatalogEntry>>>,
}

struct CatalogEntry {
    provider: ModelProviderInfo,
    models: Option<ModelsResponse>,
    home: Option<PathBuf>,
    api_key_discovery: bool,
    runtime_mode: Option<RuntimeAuthMode>,
    has_runtime_auth: bool,
    is_initial: bool,
    auth: Arc<AuthManager>,
    manager: SharedModelsManager,
}

impl CatalogEntry {
    fn matches_config(&self, config: &Config, auth: &Arc<AuthManager>) -> bool {
        Arc::ptr_eq(&self.auth, auth)
            && self.home.as_deref() == Some(config.xdcode_home.as_path())
            && self.models == config.model_catalog
            && self.api_key_discovery == config.features.enabled(Feature::ApiKeyModelDiscovery)
            && matches_catalog_provider(&self.provider, &config.model_provider)
            && (self.is_initial || !self.has_runtime_auth)
    }

    fn matches_thread(
        &self,
        config: &Config,
        models: Option<&ModelsResponse>,
        auth: &ProviderAuthContext,
    ) -> bool {
        let runtime_mode = auth.runtime_auth.as_ref().map(|(_, mode)| *mode);
        let has_runtime_auth = auth.runtime_auth.is_some();
        Arc::ptr_eq(&self.auth, &auth.credentials)
            && self.home.as_deref() == Some(config.xdcode_home.as_path())
            && self.models.as_ref() == models
            && self.api_key_discovery == config.features.enabled(Feature::ApiKeyModelDiscovery)
            && self.runtime_mode == runtime_mode
            && self.has_runtime_auth == has_runtime_auth
            && matches_catalog_provider(&self.provider, &config.model_provider)
    }
}

pub fn build_models_manager(
    config: &Config,
    credentials: Arc<AuthManager>,
    runtime: Option<Arc<RuntimeAuthManager>>,
) -> SharedModelsManager {
    let auth = ProviderAuthContext {
        credentials,
        runtime_auth: runtime
            .filter(|_| config.features.enabled(Feature::RuntimeAgentInstances))
            .map(|runtime| (runtime, config.runtime_auth_mode)),
    };
    ModelCatalogSettings::disk(config.xdcode_home.to_path_buf()).build(
        auth.model_provider(config.model_provider.clone()),
        config.model_catalog.clone(),
        config.features.enabled(Feature::ApiKeyModelDiscovery),
    )
}

fn matches_catalog_provider(expected: &ModelProviderInfo, actual: &ModelProviderInfo) -> bool {
    // Guardian overrides inference transport and retries without changing the catalog.
    // Keep all other provider fields, including routing and headers, in the comparison.
    *expected
        == ModelProviderInfo {
            request_max_retries: expected.request_max_retries,
            stream_max_retries: expected.stream_max_retries,
            supports_websockets: expected.supports_websockets,
            ..actual.clone()
        }
}

impl ThreadModelCatalog {
    pub(super) fn new(
        provider: ModelProviderInfo,
        catalog: Option<ModelsResponse>,
        factory: Arc<dyn ModelCatalogFactory>,
        api_key_model_discovery_default: bool,
        auth: ProviderAuthContext,
    ) -> Self {
        Self::new_with_home(
            provider,
            catalog,
            /*home*/ None,
            factory,
            api_key_model_discovery_default,
            auth,
        )
    }

    pub(super) fn new_with_home(
        provider: ModelProviderInfo,
        catalog: Option<ModelsResponse>,
        home: Option<PathBuf>,
        factory: Arc<dyn ModelCatalogFactory>,
        api_key_model_discovery_default: bool,
        auth: ProviderAuthContext,
    ) -> Self {
        let runtime_mode = auth.runtime_auth.as_ref().map(|(_, mode)| *mode);
        let has_runtime_auth = auth.runtime_auth.is_some();
        let credentials = Arc::clone(&auth.credentials);
        let manager = factory.build(
            auth.model_provider(provider.clone()),
            catalog.clone(),
            api_key_model_discovery_default,
        );
        let initial = CatalogEntry {
            provider: provider.clone(),
            models: catalog.clone(),
            home: home.clone(),
            api_key_discovery: api_key_model_discovery_default,
            runtime_mode,
            has_runtime_auth,
            is_initial: true,
            auth: credentials,
            manager: Arc::clone(&manager),
        };
        Self {
            manager,
            provider,
            catalog,
            home,
            factory,
            api_key_model_discovery_default,
            runtime_mode,
            entries: Arc::new(Mutex::new(vec![initial])),
        }
    }

    pub(crate) fn with_session_manager(
        config: &Config,
        auth: Arc<AuthManager>,
        manager: SharedModelsManager,
    ) -> Self {
        let home = config.xdcode_home.to_path_buf();
        let factory: Arc<dyn ModelCatalogFactory> =
            Arc::new(ModelCatalogSettings::disk(home.clone()));
        let api_key_discovery = config.features.enabled(Feature::ApiKeyModelDiscovery);
        let initial = CatalogEntry {
            provider: config.model_provider.clone(),
            models: config.model_catalog.clone(),
            home: Some(home.clone()),
            api_key_discovery,
            runtime_mode: None,
            has_runtime_auth: false,
            is_initial: true,
            auth,
            manager: Arc::clone(&manager),
        };
        Self {
            manager,
            provider: config.model_provider.clone(),
            catalog: config.model_catalog.clone(),
            home: Some(home),
            factory,
            api_key_model_discovery_default: api_key_discovery,
            runtime_mode: None,
            entries: Arc::new(Mutex::new(vec![initial])),
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
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.matches_config(config, &auth))
        {
            return Arc::clone(&entry.manager);
        }
        let api_key_discovery = config.features.enabled(Feature::ApiKeyModelDiscovery);
        let auth_context = ProviderAuthContext {
            credentials: Arc::clone(&auth),
            runtime_auth: None,
        };
        let manager = self.build_manager_for_home(
            config,
            config.model_catalog.clone(),
            auth_context,
            api_key_discovery,
        );
        if entries.len() == CATALOG_CACHE_CAPACITY {
            entries.remove(0);
        }
        entries.push(CatalogEntry {
            provider: config.model_provider.clone(),
            models: config.model_catalog.clone(),
            home: Some(config.xdcode_home.to_path_buf()),
            api_key_discovery,
            runtime_mode: None,
            has_runtime_auth: false,
            is_initial: false,
            auth,
            manager: Arc::clone(&manager),
        });
        manager
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn for_thread(
        &self,
        config: &Config,
        credentials: Arc<AuthManager>,
        runtime: Option<Arc<RuntimeAuthManager>>,
        same_owner: bool,
        originator: &str,
        session_source: &SessionSource,
    ) -> SharedModelsManager {
        let models = config.model_catalog.as_ref().or_else(|| {
            self.catalog
                .as_ref()
                .filter(|_| self.matches_catalog_provider(&config.model_provider))
        });
        let auth = ProviderAuthContext {
            credentials,
            runtime_auth: runtime.map(|runtime| (runtime, config.runtime_auth_mode)),
        };
        let manager = self.for_thread_config(config, models, auth, same_owner);
        let manager = manager.for_originator(originator).unwrap_or(manager);
        if session_source.is_non_root_agent() && config.model_catalog_id.is_none() {
            // The final provider/auth/originator catalog may be new even when the base
            // manager was reused. Preserve loaded snapshots, including unsaved updates.
            manager
                .ensure_initialized(config.http_client_factory())
                .await;
        }
        manager
    }

    pub(super) fn for_thread_config(
        &self,
        config: &Config,
        models: Option<&ModelsResponse>,
        auth: ProviderAuthContext,
        same_owner: bool,
    ) -> SharedModelsManager {
        let api_key_model_discovery_default =
            config.features.enabled(Feature::ApiKeyModelDiscovery);
        if same_owner
            && self.home.as_deref() == Some(config.xdcode_home.as_path())
            && self.runtime_mode == auth.runtime_auth.as_ref().map(|(_, mode)| *mode)
            && self.matches_catalog_provider(&config.model_provider)
            && self.catalog.as_ref() == models
            && self.api_key_model_discovery_default == api_key_model_discovery_default
        {
            return Arc::clone(&self.manager);
        }
        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = entries
                .iter()
                .find(|entry| entry.matches_thread(config, models, &auth))
            {
                return Arc::clone(&entry.manager);
            }
            let runtime_mode = auth.runtime_auth.as_ref().map(|(_, mode)| *mode);
            let has_runtime_auth = auth.runtime_auth.is_some();
            let credentials = Arc::clone(&auth.credentials);
            let manager = self.build_manager_for_home(
                config,
                models.cloned(),
                auth,
                api_key_model_discovery_default,
            );
            if entries.len() == CATALOG_CACHE_CAPACITY {
                entries.remove(0);
            }
            entries.push(CatalogEntry {
                provider: config.model_provider.clone(),
                models: models.cloned(),
                home: Some(config.xdcode_home.to_path_buf()),
                api_key_discovery: api_key_model_discovery_default,
                runtime_mode,
                has_runtime_auth,
                is_initial: false,
                auth: credentials,
                manager: Arc::clone(&manager),
            });
            manager
        }
    }

    fn build_manager_for_home(
        &self,
        config: &Config,
        models: Option<ModelsResponse>,
        auth: ProviderAuthContext,
        api_key_discovery: bool,
    ) -> SharedModelsManager {
        let same_home = self
            .home
            .as_deref()
            .is_none_or(|home| home == config.xdcode_home.as_path());
        let provider = auth.model_provider(config.model_provider.clone());
        if same_home {
            self.factory.build(provider, models, api_key_discovery)
        } else {
            ModelCatalogSettings::disk(config.xdcode_home.to_path_buf()).build(
                provider,
                models,
                api_key_discovery,
            )
        }
    }

    fn matches_catalog_provider(&self, provider: &ModelProviderInfo) -> bool {
        matches_catalog_provider(&self.provider, provider)
    }

    pub(super) fn set_api_key_model_discovery_enabled(&self, enabled: bool) {
        self.factory.set_api_key_model_discovery_enabled(enabled);
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
            xdcode_protocol::model_types::ModelOrigin::Bundled,
        ));
        let catalog = ThreadModelCatalog::with_session_manager(
            &config,
            Arc::clone(&auth),
            Arc::clone(&manager),
        );
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
