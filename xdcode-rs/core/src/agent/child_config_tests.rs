use super::*;
use crate::session::tests::make_session_and_context;
use crate::test_support::custom_provider;
use pretty_assertions::assert_eq;

async fn catalog_config() -> Config {
    let (_, turn) = make_session_and_context().await;
    let mut config = (*turn.config).clone();
    for (id, port) in [("a", 12341), ("b", 12342)] {
        config.model_providers.insert(
            id.into(),
            custom_provider(format!("http://127.0.0.1:{port}/v1")),
        );
    }
    config.models = [
        ("parent-id", "a", "parent-model"),
        ("worker-a", "a", "worker-model"),
        ("worker-b", "b", "worker-model"),
    ]
    .into_iter()
    .map(|(id, provider, model)| CatalogModel {
        id: id.into(),
        name: id.into(),
        provider: provider.into(),
        model: model.into(),
        reasoning_efforts: Some(vec![ReasoningEffort::Custom("balanced".into())]),
        default_reasoning_effort: Some(ReasoningEffort::Custom("balanced".into())),
        context_window: Some(64_000),
        auto_compact_token_limit: Some(48_000),
        compatibility: ModelCompatibility::Responses,
        ..Default::default()
    })
    .collect();
    config.model = Some("parent-model".into());
    config.model_catalog_id = Some("parent-id".into());
    config.model_provider_id = "a".into();
    config.model_provider = config.model_providers["a"].clone();
    config.model_reasoning_effort = Some(ReasoningEffort::High);
    config
}

#[tokio::test]
async fn wire_override_replaces_parent_directory_identity_without_changing_provider() {
    let mut config = catalog_config().await;
    let entry = configured_spawn_model(&config, "worker-model")
        .unwrap()
        .unwrap();
    apply_catalog_spawn_model(&mut config, &entry, None).unwrap();
    assert_eq!(config.model.as_deref(), Some("worker-model"));
    assert_eq!(config.model_catalog_id.as_deref(), Some("worker-a"));
    assert_eq!(config.model_provider_id, "a");
    assert_eq!(config.model_context_window, Some(64_000));
    assert_eq!(config.model_auto_compact_token_limit, Some(48_000));
    assert_eq!(
        config.model_reasoning_effort,
        Some(ReasoningEffort::Custom("balanced".into()))
    );
}

#[tokio::test]
async fn exact_catalog_id_selects_the_other_provider_and_its_endpoint() {
    let mut config = catalog_config().await;
    let entry = configured_spawn_model(&config, "worker-b")
        .unwrap()
        .unwrap();
    apply_catalog_spawn_model(&mut config, &entry, None).unwrap();
    assert_eq!(config.model_catalog_id.as_deref(), Some("worker-b"));
    assert_eq!(config.model.as_deref(), Some("worker-model"));
    assert_eq!(config.model_provider_id, "b");
    assert_eq!(
        config.model_provider.base_url.as_deref(),
        Some("http://127.0.0.1:12342/v1")
    );
}

#[tokio::test]
async fn ambiguous_wire_override_requires_catalog_identity() {
    let mut config = catalog_config().await;
    let mut duplicate = config.models[1].clone();
    duplicate.id = "worker-a-alias".into();
    config.models.push(duplicate);
    assert!(configured_spawn_model(&config, "worker-model").is_err());
    let entry = configured_spawn_model(&config, "worker-a-alias")
        .unwrap()
        .unwrap();
    apply_catalog_spawn_model(&mut config, &entry, None).unwrap();
    assert_eq!(
        configured_spawn_model(&config, "worker-model")
            .unwrap()
            .unwrap()
            .id,
        "worker-a-alias"
    );
}

#[tokio::test]
async fn service_default_effort_does_not_inherit_parent_effort() {
    let mut config = catalog_config().await;
    let mut entry = config.models[1].clone();
    entry.reasoning_efforts = Some(vec![]);
    entry.default_reasoning_effort = None;
    apply_catalog_spawn_model(&mut config, &entry, None).unwrap();
    assert_eq!(config.model_reasoning_effort, None);
    assert!(validate_catalog_spawn_effort(&entry, Some(&ReasoningEffort::None)).is_err());
}

#[tokio::test]
async fn unsupported_effort_does_not_apply_a_partial_catalog_selection() {
    let mut config = catalog_config().await;
    let before = config.clone();
    let entry = config.models[2].clone();
    assert!(apply_catalog_spawn_model(&mut config, &entry, Some(ReasoningEffort::High)).is_err());
    assert_eq!(config, before);
}

#[tokio::test]
async fn incompatible_catalog_entry_is_rejected() {
    let mut config = catalog_config().await;
    let before = config.clone();
    let mut entry = config.models[2].clone();
    entry.compatibility = ModelCompatibility::Incompatible;
    assert!(apply_catalog_spawn_model(&mut config, &entry, None).is_err());
    assert_eq!(config, before);
}

#[tokio::test]
async fn role_or_legacy_model_override_cannot_keep_an_unrelated_catalog_id() {
    let mut config = catalog_config().await;
    config.model = Some("role-model".into());
    retain_matching_catalog_identity(&mut config);
    assert_eq!(config.model.as_deref(), Some("role-model"));
    assert_eq!(config.model_catalog_id, None);
}
