use super::*;
use crate::session::tests::make_session_and_context;
use crate::test_support::custom_provider;
use pretty_assertions::assert_eq;
use xdcode_config::model_catalog::ModelCompatibility;

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
async fn role_or_legacy_model_override_cannot_keep_an_unrelated_catalog_id() {
    let mut config = catalog_config().await;
    config.model = Some("role-model".into());
    retain_matching_catalog_identity(&mut config);
    assert_eq!(config.model.as_deref(), Some("role-model"));
    assert_eq!(config.model_catalog_id, None);
}
