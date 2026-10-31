use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::sync::Notify;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing_subscriber::layer::SubscriberExt;
use xdcode_config::CloudConfigBundle;
use xdcode_config::CloudConfigBundleLoader;
use xdcode_config::LoaderOverrides;
use xdcode_config::test_support::CloudConfigBundleFixture;
use xdcode_core::config::ConfigBuilder;
use xdcode_core::config::ConfigOverrides;
use xdcode_feedback::XdcodeFeedback;
use xdcode_http_client::HttpClientFactory;
use xdcode_models_manager::manager::ModelsEndpointClient;
use xdcode_models_manager::manager::ModelsEndpointFuture;
use xdcode_models_manager::manager::ModelsEndpointResponse;
use xdcode_models_manager::manager::ProviderModelsManager;
use xdcode_models_manager::manager::SharedModelsManager;
use xdcode_protocol::error::Result as CoreResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_utils_absolute_path::AbsolutePathBuf;

use super::*;

#[derive(Debug)]
struct TestModelsEndpoint {
    fetch_count: AtomicUsize,
    fetched: Notify,
    release_second_fetch: Notify,
}

impl TestModelsEndpoint {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fetch_count: AtomicUsize::new(0),
            fetched: Notify::new(),
            release_second_fetch: Notify::new(),
        })
    }

    async fn wait_for_fetch_count(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.fetch_count.load(Ordering::SeqCst) < expected {
                self.fetched.notified().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("expected {expected} model fetches"));
    }
}

impl ModelsEndpointClient for TestModelsEndpoint {
    fn identity(&self) -> Option<String> {
        Some("test-provider".to_string())
    }

    fn has_command_auth(&self) -> bool {
        true
    }

    fn uses_xdcode_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(async { false })
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<ModelsEndpointResponse>> {
        Box::pin(async move {
            let fetch_index = self.fetch_count.fetch_add(1, Ordering::SeqCst);
            self.fetched.notify_one();
            if fetch_index == 0 {
                return Err(XdcodeErr::Io(std::io::Error::other("test failure")));
            }
            if fetch_index == 1 {
                self.release_second_fetch.notified().await;
            }
            Ok(ModelsEndpointResponse {
                models: Vec::new(),
                etag: None,
                identity: self.identity().expect("test endpoint identity"),
            })
        })
    }
}

#[tokio::test]
async fn refreshes_immediately_periodically_and_stops_when_dropped() {
    let xdcode_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new();
    let requirements_path = xdcode_home.path().join("requirements.toml");
    std::fs::write(&requirements_path, "model_provider = 'ollama'").unwrap();
    let config_manager = crate::config_manager::ConfigManager::new_for_tests(
        xdcode_home.path().to_path_buf(),
        vec![("model_provider".to_string(), "ollama".into())],
        LoaderOverrides::with_managed_config_path_for_tests(
            xdcode_home.path().join("managed_config.toml"),
        ),
        CloudConfigBundleLoader::default(),
    );
    // The TUI forwards its typed --oss selection through the raw CLI overrides too.
    let config = Arc::new(
        ConfigBuilder::default()
            .xdcode_home(xdcode_home.path().to_path_buf())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .harness_overrides(ConfigOverrides {
                model_provider: Some("ollama".to_string()),
                ..Default::default()
            })
            .build()
            .await
            .unwrap(),
    );
    let models_manager: SharedModelsManager = Arc::new(ProviderModelsManager::new(
        xdcode_home.path().to_path_buf(),
        endpoint.clone(),
        /*auth_manager*/ None,
    ));
    let auth_manager = xdcode_login::AuthManager::shared_from_config(
        config.as_ref(),
        /*enable_xdcode_api_key_env*/ false,
    )
    .await
    .expect("test auth manager");
    let catalog = Arc::new(ModelCatalog::new(
        config_manager,
        Some(config.cwd.to_path_buf()),
        config,
        Arc::clone(&models_manager),
        auth_manager,
    ));
    let worker = spawn_with_interval(&catalog, Duration::from_millis(/*millis*/ 10));

    endpoint.wait_for_fetch_count(/*expected*/ 2).await;
    drop(worker);
    endpoint.release_second_fetch.notify_one();
    tokio::time::sleep(Duration::from_millis(30)).await;

    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 2);

    // Reloading after login preserves an embedded caller's typed --oss override.
    catalog.invalidate().await;
    assert_eq!(
        catalog.current().await.unwrap().0.model_provider_id,
        "ollama"
    );

    assert!(Arc::ptr_eq(
        &catalog.current().await.unwrap().1,
        &models_manager
    ));

    // The catalog follows a new managed route without reusing the old endpoint.
    std::fs::write(&requirements_path, "model_provider = 'provider'").unwrap();
    catalog.invalidate().await;
    catalog.list_models(RefreshStrategy::Offline).await.unwrap();
    assert_eq!(
        catalog.current().await.unwrap().0.model_provider_id,
        "provider"
    );
    std::fs::remove_file(&requirements_path).unwrap();
    catalog.invalidate().await;
    assert_eq!(
        catalog.current().await.unwrap().0.model_provider_id,
        "ollama"
    );
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn background_refresh_blocks_invalid_requirements_then_recovers() -> anyhow::Result<()> {
    let home = tempdir()?;
    let config = Arc::new(
        crate::config_manager::ConfigManager::without_managed_config_for_tests(
            home.path().to_path_buf(),
        )
        .load_non_project_config()
        .await?,
    );
    let (checks_tx, mut checks_rx) = mpsc::unbounded_channel();
    let loader = CloudConfigBundleLoader::from_getter(move || {
        let (reply_tx, reply_rx) = oneshot::channel::<Option<CloudConfigBundle>>();
        checks_tx
            .send(reply_tx)
            .expect("requirements check receiver");
        async move { Ok(reply_rx.await.expect("requirements check response")) }
    });
    let config_manager = crate::config_manager::ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        loader,
    );
    let endpoint = TestModelsEndpoint::new();
    let models_manager: SharedModelsManager = Arc::new(ProviderModelsManager::new(
        home.path().to_path_buf(),
        endpoint.clone(),
        /*auth_manager*/ None,
    ));
    let auth_manager = xdcode_login::AuthManager::shared_from_config(
        config.as_ref(),
        /*enable_xdcode_api_key_env*/ false,
    )
    .await
    .expect("test auth manager");
    let catalog = Arc::new(ModelCatalog::new(
        config_manager,
        Some(config.cwd.to_path_buf()),
        config,
        models_manager,
        auth_manager,
    ));
    let feedback = XdcodeFeedback::new();
    // This test uses Tokio's current-thread runtime, so spawned refreshes share the subscriber.
    let _subscriber_guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(feedback.logger_layer()),
    );
    let worker = spawn_with_interval(&catalog, Duration::from_millis(/*millis*/ 10));
    tokio::time::timeout(Duration::from_secs(/*secs*/ 10), async {
        for expected in 1..=2 {
            checks_rx.recv().await.unwrap().send(/*t*/ None).unwrap();
            endpoint.wait_for_fetch_count(expected).await;
        }

        // Let the in-flight request finish, then control each refresh's requirements.
        endpoint.release_second_fetch.notify_one();
        let mut next_check = checks_rx.recv().await.unwrap();
        for requirements in [
            "model_provider = 'other'",
            "model_provider = []",
            "[model_providers.gateway]\nexperimental_bearer_token = 'private-provider-token-marker' trailing",
        ] {
            next_check
                .send(Some(
                    CloudConfigBundleFixture::enterprise_requirement(requirements).into_bundle(),
                ))
                .unwrap();
            // Reaching the next check proves the previous refresh completed. Keep
            // this check blocked while asserting that the previous one did not fetch.
            next_check = checks_rx.recv().await.unwrap();
            assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 2);
        }

        next_check.send(/*t*/ None).unwrap();
        endpoint.wait_for_fetch_count(/*expected*/ 3).await;
    })
    .await?;
    drop(worker);
    let logs = String::from_utf8(
        feedback
            .snapshot(/*session_id*/ None)
            .log_attachment(/*logs_override*/ None)
            .buffer,
    )?;
    assert!(logs.contains("model catalog refresh blocked by provider requirements"));
    assert!(logs.contains("error_kind=InvalidData"));
    assert!(!logs.contains("private-provider-token-marker"));
    Ok(())
}

/// A transient cloud failure must not pin the fallback catalog after policy recovers.
#[tokio::test]
async fn catalog_reload_recovers_after_cloud_failure() -> anyhow::Result<()> {
    let home = tempdir()?;
    let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let loader_failed = Arc::clone(&failed);
    let loader = CloudConfigBundleLoader::from_getter(move || {
        let failed = Arc::clone(&loader_failed);
        async move {
            if failed.load(Ordering::SeqCst) {
                Err(xdcode_config::CloudConfigBundleLoadError::new(
                    xdcode_config::CloudConfigBundleLoadErrorCode::RequestFailed,
                    /*status_code*/ None,
                    "test cloud timeout",
                ))
            } else {
                Ok(Some(
                    CloudConfigBundleFixture::enterprise_requirement("model_provider = 'ollama'")
                        .into_bundle(),
                ))
            }
        }
    });
    let manager = crate::config_manager::ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        loader,
    );
    let config = Arc::new(manager.load_non_project_config().await?);
    let auth = xdcode_login::AuthManager::shared_from_config(
        config.as_ref(),
        /*enable_xdcode_api_key_env*/ false,
    )
    .await?;
    let models = xdcode_core::build_models_manager(&config, Arc::clone(&auth));
    let catalog = Arc::new(ModelCatalog::new(
        manager, /*startup_cwd*/ None, config, models, auth,
    ));
    failed.store(true, Ordering::SeqCst);
    catalog.invalidate().await;
    assert_eq!(catalog.current().await?.0.model_provider_id, "provider");
    assert!(catalog.list_models(RefreshStrategy::Offline).await.is_err());
    failed.store(false, Ordering::SeqCst);
    // Exercise the same online request used by the periodic background worker.
    catalog.list_models(RefreshStrategy::Online).await?;
    assert_eq!(catalog.current().await?.0.model_provider_id, "ollama");
    assert!(
        !catalog
            .list_models(RefreshStrategy::Offline)
            .await?
            .is_empty()
    );
    Ok(())
}

/// Repairing a project config after fallback must restore that project's local model catalog.
#[tokio::test]
async fn catalog_reload_recovers_original_project_after_startup_fallback() -> anyhow::Result<()> {
    let home = tempdir()?;
    let project = tempdir()?;
    let project_path = AbsolutePathBuf::from_absolute_path(project.path())?
        .canonicalize()?
        .to_path_buf();
    std::fs::create_dir(project_path.join(".xdcode"))?;
    std::fs::write(
        home.path().join("config.toml"),
        toml::to_string(&serde_json::json!({
            "projects": {project_path.to_string_lossy(): {"trust_level": "trusted"}}
        }))?,
    )?;
    let project_config = project_path.join(".xdcode/config.toml");
    std::fs::write(&project_config, "model_catalog_json = [")?;
    let manager = crate::config_manager::ConfigManager::without_managed_config_for_tests(
        home.path().to_path_buf(),
    );
    let startup = manager
        .load_startup_config(Some(project_path.clone()))
        .await?;
    assert!(startup.used_defaults);
    let config = Arc::new(startup.config);
    let auth = xdcode_login::AuthManager::shared_from_config(
        config.as_ref(),
        /*enable_xdcode_api_key_env*/ false,
    )
    .await?;
    let models = xdcode_core::build_models_manager(&config, Arc::clone(&auth));
    let catalog = ModelCatalog::new(manager, Some(project_path.clone()), config, models, auth);
    let mut model = xdcode_models_manager::bundled_models_response()?
        .models
        .remove(0);
    model.slug = "project-model".into();
    std::fs::write(
        project_path.join(".xdcode/catalog.json"),
        serde_json::to_vec(&serde_json::json!({"models": [model]}))?,
    )?;
    std::fs::write(&project_config, "model_catalog_json = 'catalog.json'")?;
    catalog.invalidate().await;
    let (config, _) = catalog.current().await?;
    assert_eq!(config.cwd.as_path(), project_path);
    let models = catalog.list_models(RefreshStrategy::Offline).await?;
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["project-model"]
    );
    Ok(())
}
