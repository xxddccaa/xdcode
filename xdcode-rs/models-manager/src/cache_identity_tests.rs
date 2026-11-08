//! Coverage for cache identity validation and conditional freshness updates.

use super::*;

#[tokio::test]
async fn initialization_preserves_loaded_and_unsaved_catalogs() {
    for persistence_fails in [false, true] {
        let loaded = remote_model("loaded", "Loaded", 0);
        let replacement = remote_model("replacement", "Replacement", 0);
        let cache = TestModelsCache::failing(false, persistence_fails);
        let endpoint =
            TestModelsEndpoint::without_refresh(vec![vec![loaded.clone()], vec![replacement]]);
        let manager = ProviderModelsManager::new_with_cache(cache, endpoint.clone(), None)
            .with_provider_catalog();
        manager
            .raw_model_catalog(RefreshStrategy::Online, DEFAULT_HTTP_CLIENT_FACTORY)
            .await;
        manager
            .ensure_initialized(DEFAULT_HTTP_CLIENT_FACTORY)
            .await;
        assert_eq!(endpoint.fetch_count(), 1);
        assert_eq!(manager.remote_models.read().await.models, vec![loaded]);
    }
}

#[tokio::test]
async fn concurrent_initialization_fetches_only_once_even_for_empty_catalogs() {
    for models in [Vec::new(), vec![remote_model("child", "Child", 0)]] {
        let endpoint = TestModelsEndpoint::without_refresh(vec![models.clone()]);
        let manager = Arc::new(
            ProviderModelsManager::new_without_cache(endpoint.clone(), None)
                .with_provider_catalog(),
        );
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let manager = Arc::clone(&manager);
            tasks.spawn(async move {
                manager
                    .ensure_initialized(DEFAULT_HTTP_CLIENT_FACTORY)
                    .await
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.expect("initialization task");
        }
        assert_eq!(endpoint.fetch_count(), 1);
        assert_eq!(manager.remote_models.read().await.models, models);
    }
}

#[tokio::test]
async fn initialization_reuses_a_valid_cache_without_fetching() {
    let models = vec![remote_model("cached-child", "Cached child", 0)];
    let cache = TestModelsCache::with_entry(ModelsCacheEntry {
        identity: Some("test-provider".into()),
        fetched_at: Utc::now(),
        etag: None,
        client_version: Some(crate::client_version_to_whole()),
        models: models.clone(),
    });
    let endpoint = TestModelsEndpoint::without_refresh(Vec::new());
    let manager = ProviderModelsManager::new_with_cache(cache, endpoint.clone(), None)
        .with_provider_catalog();
    manager
        .ensure_initialized(DEFAULT_HTTP_CLIENT_FACTORY)
        .await;
    assert_eq!(endpoint.fetch_count(), 0);
    assert_eq!(manager.remote_models.read().await.models, models);
}

#[tokio::test]
async fn disabled_discovery_does_not_initialize_an_api_key_provider() {
    let endpoint =
        TestModelsEndpoint::without_refresh(vec![vec![remote_model("private", "Private", 0)]]);
    let auth =
        AuthManager::from_auth_for_testing(XdcodeAuth::from_api_key("test-initialization-key"));
    let manager = ProviderModelsManager::new_without_cache(endpoint.clone(), Some(auth))
        .with_provider_catalog();
    manager
        .ensure_initialized(DEFAULT_HTTP_CLIENT_FACTORY)
        .await;
    assert_eq!(endpoint.fetch_count(), 0);
    assert!(!manager.catalog_initialized.load(Ordering::Acquire));
}
use pretty_assertions::assert_eq;

#[tokio::test]
async fn mismatched_and_legacy_cache_entries_fetch_the_current_catalog() {
    for identity in [None, Some("other-provider-or-account".to_string())] {
        let stale = remote_model("stale", "Stale", /*priority*/ 0);
        let current = remote_model("current", "Current", /*priority*/ 0);
        let cache = TestModelsCache::with_entry(ModelsCacheEntry {
            fetched_at: Utc::now(),
            etag: Some("old-etag".into()),
            client_version: Some(crate::client_version_to_whole()),
            identity,
            models: vec![stale],
        });
        let endpoint = TestModelsEndpoint::new(vec![vec![current.clone()]]);
        let manager = ProviderModelsManager::new_with_cache(
            cache.clone(),
            endpoint.clone(),
            Some(AuthManager::from_auth_for_testing(
                XdcodeAuth::create_dummy_chatgpt_auth_for_testing(),
            )),
        );
        assert_eq!(
            manager
                .raw_model_catalog(
                    RefreshStrategy::OnlineIfUncached,
                    DEFAULT_HTTP_CLIENT_FACTORY
                )
                .await
                .models,
            vec![current.clone()]
        );
        assert_eq!(endpoint.fetch_count(), 1);
    }
}

#[tokio::test]
async fn matching_etag_cannot_renew_a_different_identity_or_catalog() {
    let home = tempdir().unwrap();
    let cache = FileModelsCache::new(home.path().join(MODEL_CACHE_FILE), DEFAULT_MODEL_CACHE_TTL);
    let expected = ModelsCacheEntry {
        fetched_at: Utc::now() - chrono::Duration::hours(1),
        etag: Some("etag".into()),
        client_version: Some(crate::client_version_to_whole()),
        identity: Some("first-account".into()),
        models: vec![remote_model("model", "Model", /*priority*/ 0)],
    };
    for (key, etag, version) in [
        ("other-account", "etag", crate::client_version_to_whole()),
        (
            "first-account",
            "new-etag",
            crate::client_version_to_whole(),
        ),
        ("first-account", "etag", "other-version".into()),
    ] {
        let stored = ModelsCacheEntry {
            identity: Some(key.into()),
            etag: Some(etag.into()),
            client_version: Some(version),
            ..expected.clone()
        };
        cache.store(&stored).await.unwrap();
        cache
            .refresh_ttl(
                expected.client_version.as_deref().unwrap(),
                expected.identity.as_deref().unwrap(),
                expected.etag.as_deref().unwrap(),
            )
            .await
            .unwrap();
        let actual: ModelsCacheEntry = serde_json::from_slice(
            &tokio::fs::read(home.path().join(MODEL_CACHE_FILE))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual, stored);
    }
}
