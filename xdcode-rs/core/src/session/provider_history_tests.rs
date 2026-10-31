use super::*;
use crate::session::session::SessionSettingsUpdate;
use std::sync::Arc;
use xdcode_config::ConstraintError;
use xdcode_config::model_catalog::CatalogModel;
use xdcode_history::CompactedItem;
use xdcode_history::ResumedHistory;
use xdcode_protocol::ThreadId;
use xdcode_protocol::config_types::ModeKind;
use xdcode_protocol::models::ResponseItem;
use xdcode_protocol::protocol::CatalogModelSelection;
use xdcode_protocol::protocol::SessionMeta;
use xdcode_protocol::protocol::SessionMetaLine;

#[tokio::test]
async fn rejected_provider_history_keeps_settings_and_allows_same_provider_plan() {
    for item in [
        opaque(),
        serde_json::from_value(serde_json::json!({
            "type": "compaction", "encrypted_content": "provider-a-checkpoint"
        }))
        .unwrap(),
    ] {
        let (session, turn, _events) =
            crate::session::tests::make_session_and_context_with_auth_and_config_and_rx(
                xdcode_login::XdcodeAuth::from_api_key("history-test-credential"),
                Vec::new(),
                |config| {
                    config.model_provider.env_key = None;
                    config.model_provider.experimental_bearer_token =
                        Some("history-test-credential".into());
                },
            )
            .await;
        let original_provider = turn.provider.clone();
        let model = turn.model_info().slug.clone();
        let provider_id;
        {
            let mut state = session.state.lock().await;
            let mut config = (*state.session_configuration.original_config_do_not_use).clone();
            config.config_layer_stack = xdcode_config::ConfigLayerStack::default();
            provider_id = config.model_provider_id.clone();
            config
                .model_providers
                .insert(provider_id.clone(), config.model_provider.clone());
            let mut other = config.model_provider.clone();
            other.base_url = Some("http://127.0.0.1:12345/v1".into());
            other.env_key = None;
            config
                .model_providers
                .insert("history-provider-b".into(), other);
            config.models = [
                ("history-current", provider_id.clone()),
                ("history-other", "history-provider-b".into()),
            ]
            .into_iter()
            .map(|(id, provider)| CatalogModel {
                id: id.into(),
                name: id.into(),
                provider,
                model: model.clone(),
                reasoning_efforts: Some(vec![]),
                ..Default::default()
            })
            .collect();
            state.session_configuration.original_config_do_not_use = Arc::new(config);
            state
                .history
                .record_items([&item], TruncationPolicy::Bytes(usize::MAX / 2));
        }
        let initial = session.thread_settings_snapshot().await;
        let initial_config = session.get_config().await;
        let initial_history = session.clone_history().await.annotated_items().to_vec();
        let error = session
            .update_settings(SessionSettingsUpdate {
                catalog_selection: Some(CatalogModelSelection {
                    provider: "history-provider-b".into(),
                    catalog_id: "history-other".into(),
                    model: model.clone(),
                    effort: None,
                    mode: ModeKind::Plan,
                }),
                ..Default::default()
            })
            .await
            .err()
            .expect("opaque history must reject a provider change");
        assert!(matches!(
            &error,
            ConstraintError::ProviderHistoryNotPortable { .. }
        ));
        assert!(!error.to_string().contains("provider-a-private"));
        assert!(!error.to_string().contains("provider-a-checkpoint"));
        assert_eq!(session.thread_settings_snapshot().await, initial);
        assert!(Arc::ptr_eq(&session.get_config().await, &initial_config));
        assert_eq!(
            session.clone_history().await.annotated_items(),
            &initial_history
        );
        assert_eq!(
            session.new_default_turn().await.provider.info(),
            original_provider.info()
        );

        session
            .update_settings(SessionSettingsUpdate {
                catalog_selection: Some(CatalogModelSelection {
                    provider: provider_id.clone(),
                    catalog_id: "history-current".into(),
                    model,
                    effort: None,
                    mode: ModeKind::Plan,
                }),
                ..Default::default()
            })
            .await
            .expect("the current provider can enter Plan with its own encrypted history");
        let applied = session.thread_settings_snapshot().await;
        assert_eq!(applied.model_provider_id, provider_id);
        assert_eq!(applied.collaboration_mode.mode, ModeKind::Plan);
        assert_eq!(
            session.clone_history().await.annotated_items(),
            &initial_history
        );
        assert_eq!(turn.provider.info(), original_provider.info());
    }
}

fn meta(provider: Option<&str>) -> RolloutItem {
    RolloutItem::SessionMeta(Box::new(SessionMetaLine {
        meta: SessionMeta {
            model_provider: provider.map(str::to_owned),
            ..Default::default()
        },
        git: None,
    }))
}

fn opaque() -> ResponseItem {
    serde_json::from_value(serde_json::json!({
        "type": "reasoning", "summary": [], "encrypted_content": "provider-a-private"
    }))
    .unwrap()
}

fn history(items: Vec<RolloutItem>, fork: bool) -> InitialHistory {
    if fork {
        InitialHistory::Forked(items)
    } else {
        InitialHistory::Resumed(ResumedHistory {
            conversation_id: ThreadId::default(),
            history: Arc::new(items),
            history_revision: None,
            rollout_path: None,
        })
    }
}

#[test]
fn legacy_opaque_resume_and_fork_require_verified_endpoint() {
    for fork in [false, true] {
        let history = history(
            vec![meta(Some("a")), RolloutItem::ResponseItem(opaque().into())],
            fork,
        );
        assert!(validate_initial_history(&history, "a", "endpoint-a").is_err());
        let before = serde_json::to_value(&history).unwrap();
        let error = validate_initial_history(&history, "b", "endpoint-b").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("saved history has been preserved")
        );
        assert_eq!(serde_json::to_value(history).unwrap(), before);
    }
}

#[test]
fn cross_provider_resume_and_fork_accept_readable_history() {
    for fork in [false, true] {
        let text = serde_json::from_value::<ResponseItem>(serde_json::json!({
            "type": "message", "role": "user", "content": [{"type":"input_text", "text":"keep context"}]
        }))
        .unwrap();
        let history = history(
            vec![meta(Some("a")), RolloutItem::ResponseItem(text.into())],
            fork,
        );
        assert!(validate_initial_history(&history, "b", "endpoint-b").is_ok());
    }
}

#[test]
fn latest_provider_does_not_relabel_old_ciphertext() {
    let history = history(
        vec![
            meta(Some("a")),
            RolloutItem::ResponseItem(opaque().into()),
            meta(Some("b")),
        ],
        false,
    );
    assert!(validate_initial_history(&history, "b", "endpoint-b").is_err());
}

#[test]
fn unknown_provider_opaque_history_requires_portable_checkpoint() {
    let history = history(vec![RolloutItem::ResponseItem(opaque().into())], false);
    assert!(validate_initial_history(&history, "a", "endpoint-a").is_err());
}

#[test]
fn cross_provider_compaction_replacement_is_checked() {
    let compacted: CompactedItem = serde_json::from_value(serde_json::json!({
        "message": "summary",
        "replacement_history": [opaque()]
    }))
    .unwrap();
    let history = history(
        vec![meta(Some("a")), RolloutItem::Compacted(Box::new(compacted))],
        true,
    );
    assert!(validate_initial_history(&history, "b", "endpoint-b").is_err());
    assert!(validate_initial_history(&history, "a", "endpoint-a").is_err());
}

#[tokio::test]
async fn settings_provider_attribution_does_not_relabel_earlier_ciphertext() {
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let mut settings = session.thread_settings_snapshot().await;
    settings.model_provider_id = "b".into();
    settings.provider_fingerprint = Some("endpoint-b".into());
    let event = RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
        xdcode_protocol::protocol::ThreadSettingsAppliedEvent {
            thread_id: Some(session.thread_id()),
            thread_settings: settings,
        },
    ));
    for fork in [false, true] {
        let old_ciphertext = history(
            vec![
                meta(Some("a")),
                RolloutItem::ResponseItem(opaque().into()),
                event.clone(),
            ],
            fork,
        );
        assert!(validate_initial_history(&old_ciphertext, "b", "endpoint-b").is_err());
        let owned_by_b = history(
            vec![
                meta(Some("a")),
                event.clone(),
                RolloutItem::ResponseItem(opaque().into()),
            ],
            fork,
        );
        assert!(validate_initial_history(&owned_by_b, "b", "endpoint-b").is_ok());
        assert!(validate_initial_history(&owned_by_b, "a", "endpoint-a").is_err());
    }
}

#[tokio::test]
async fn same_provider_history_requires_the_same_recorded_endpoint() {
    let (session, _) = crate::session::tests::make_session_and_context().await;
    let mut settings = session.thread_settings_snapshot().await;
    settings.model_provider_id = "a".into();
    settings.provider_fingerprint = Some("endpoint-a".into());
    for fork in [false, true] {
        let saved = history(
            vec![
                meta(Some("a")),
                RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
                    xdcode_protocol::protocol::ThreadSettingsAppliedEvent {
                        thread_id: Some(session.thread_id()),
                        thread_settings: settings.clone(),
                    },
                )),
                RolloutItem::ResponseItem(opaque().into()),
            ],
            fork,
        );
        assert!(validate_initial_history(&saved, "a", "endpoint-a").is_ok());
        assert!(validate_initial_history(&saved, "a", "changed-base-url").is_err());
        assert!(validate_initial_history(&saved, "b", "endpoint-a").is_err());
    }
}

#[tokio::test]
async fn mid_turn_settings_do_not_relabel_late_original_provider_ciphertext() {
    let (session, turn) = crate::session::tests::make_session_and_context().await;
    let mut a_context = turn.to_turn_context_item();
    a_context.model_provider_id = Some("a".into());
    a_context.provider_fingerprint = Some("endpoint-a".into());
    let mut b_settings = session.thread_settings_snapshot().await;
    b_settings.model_provider_id = "b".into();
    b_settings.provider_fingerprint = Some("endpoint-b".into());
    let history = history(
        vec![
            meta(Some("a")),
            RolloutItem::TurnContext(a_context),
            RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
                xdcode_protocol::protocol::ThreadSettingsAppliedEvent {
                    thread_id: Some(session.thread_id()),
                    thread_settings: b_settings,
                },
            )),
            RolloutItem::ResponseItem(opaque().into()),
        ],
        false,
    );
    assert!(validate_initial_history(&history, "b", "endpoint-b").is_err());
    assert!(validate_initial_history(&history, "a", "endpoint-a").is_ok());
}

#[test]
fn portable_compaction_retires_old_provider_ciphertext_for_resume_and_fork() {
    for fork in [false, true] {
        let compacted: CompactedItem = serde_json::from_value(serde_json::json!({
            "message": "portable summary", "replacement_history": [{
                "type": "message", "role": "user", "content": [{
                    "type": "input_text", "text": "portable checkpoint"
                }]
            }]
        }))
        .unwrap();
        let saved = history(
            vec![
                meta(Some("a")),
                RolloutItem::ResponseItem(opaque().into()),
                RolloutItem::Compacted(Box::new(compacted)),
            ],
            fork,
        );
        assert!(validate_initial_history(&saved, "b", "endpoint-b").is_ok());
    }
}

#[tokio::test]
async fn pre_turn_compaction_records_its_pinned_provider_before_ciphertext() {
    let (session, turn) = crate::session::tests::make_session_and_context().await;
    let mut a_context = turn.to_turn_context_item();
    a_context.model_provider_id = Some("a".into());
    a_context.provider_fingerprint = Some("endpoint-a".into());
    let mut b_context = a_context.clone();
    b_context.model_provider_id = Some("b".into());
    b_context.provider_fingerprint = Some("endpoint-b".into());
    let mut b_settings = session.thread_settings_snapshot().await;
    b_settings.model_provider_id = "b".into();
    b_settings.provider_fingerprint = Some("endpoint-b".into());
    let compacted: CompactedItem = serde_json::from_value(serde_json::json!({
        "message": "provider B checkpoint", "replacement_history": [{
            "type": "compaction", "encrypted_content": "provider-b-checkpoint"
        }]
    }))
    .unwrap();
    let original_order = vec![
        meta(Some("a")),
        RolloutItem::TurnContext(a_context),
        RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
            xdcode_protocol::protocol::ThreadSettingsAppliedEvent {
                thread_id: Some(session.thread_id()),
                thread_settings: b_settings,
            },
        )),
        RolloutItem::Compacted(Box::new(compacted)),
        RolloutItem::TurnContext(b_context.clone()),
    ];
    let mut pinned_order = original_order.clone();
    pinned_order.insert(3, RolloutItem::TurnContext(b_context));
    let replay = |items: &[RolloutItem]| {
        ContextManager::reconstruct_rollout(
            items,
            ThreadHistoryMode::default(),
            ContextManager::new(),
            TruncationPolicy::Bytes(usize::MAX / 2),
        )
    };
    // The added preamble changes provenance without changing checkpoint replay.
    assert_eq!(replay(&original_order), replay(&pinned_order));
    for fork in [false, true] {
        assert!(
            validate_initial_history(&history(original_order.clone(), fork), "b", "endpoint-b")
                .is_err()
        );
        assert!(
            validate_initial_history(&history(pinned_order.clone(), fork), "b", "endpoint-b")
                .is_ok()
        );
        assert!(
            validate_initial_history(&history(pinned_order.clone(), fork), "a", "endpoint-a")
                .is_err()
        );
    }
}
