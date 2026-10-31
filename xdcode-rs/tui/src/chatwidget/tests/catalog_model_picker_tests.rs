//! Behavioral catalog tests: routing choices remain atomic and browsing is side-effect free.
use super::*;
use xdcode_config::model_catalog::CatalogModel;
use xdcode_protocol::protocol::CatalogModelSelection;

fn model(id: &str, provider: &str, efforts: Vec<ReasoningEffortConfig>) -> CatalogModel {
    CatalogModel {
        id: id.into(),
        name: "Shared display name".into(),
        provider: provider.into(),
        model: "shared-request-model".into(),
        reasoning_efforts: Some(efforts),
        ..Default::default()
    }
}

#[tokio::test]
async fn custom_reasoning_choice_sends_one_atomic_routing_event_without_mutating_widget() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some("original-model")).await;
    let provider = chat.config.model_provider_id.clone();
    let effort = ReasoningEffortConfig::Custom("vendor-level".into());
    chat.open_catalog_reasoning(model("second-id", "second-provider", vec![effort.clone()]));
    // The first row is Service default; the second retains the exact custom value.
    chat.handle_key_event(KeyCode::Down.into());
    chat.handle_key_event(KeyCode::Enter.into());
    let selection = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::SelectCatalogModel {
                selection,
                persist: true,
            } => Some(selection),
            _ => None,
        })
        .expect("atomic selection event");
    assert_eq!(selection.provider, "second-provider");
    assert_eq!(selection.catalog_id, "second-id");
    assert_eq!(selection.effort, Some(effort));
    assert_eq!(chat.current_model(), "original-model");
    assert_eq!(chat.config.model_provider_id, provider);
}

#[tokio::test]
async fn service_default_and_literal_none_are_distinct_picker_actions() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(None).await;
    let entry = model("same", "provider", vec![ReasoningEffortConfig::None]);
    chat.open_catalog_reasoning(entry.clone());
    chat.handle_key_event(KeyCode::Enter.into());
    let first = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| {
            if let AppEvent::SelectCatalogModel { selection, .. } = event {
                Some(selection)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(first.effort, None);
    chat.open_catalog_reasoning(entry);
    chat.handle_key_event(KeyCode::Down.into());
    chat.handle_key_event(KeyCode::Enter.into());
    let second = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| {
            if let AppEvent::SelectCatalogModel { selection, .. } = event {
                Some(selection)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(second.effort, Some(ReasoningEffortConfig::None));
}

#[tokio::test]
async fn confirmed_plan_choice_leaves_normal_model_intact() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("normal-model")).await;
    let selection = CatalogModelSelection {
        provider: "second-provider".into(),
        catalog_id: "plan-id".into(),
        model: "plan-model".into(),
        effort: None,
        mode: ModeKind::Plan,
    };
    chat.apply_confirmed_catalog_selection(&selection);
    assert_eq!(chat.effective_collaboration_mode().mode, ModeKind::Plan);
    assert_eq!(chat.current_model(), "plan-model");
    assert_eq!(chat.current_collaboration_mode.model(), "normal-model");
    assert_eq!(chat.current_reasoning_effort(), None);
}

#[tokio::test]
async fn opening_catalog_sees_models_added_after_widget_creation() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("original-model")).await;
    let file = chat.config.xdcode_home.join("config.toml");
    let initial = r#"
[[models]]
id = "first"
name = "Initial choice"
provider = "local"
model = "first-wire"
reasoning_efforts = []
[model_providers.local]
name = "Local"
base_url = "http://127.0.0.1:1/v1"
wire_api = "responses"
"#;
    std::fs::write(&file, initial).unwrap();
    chat.config.config_layer_stack = chat
        .config
        .config_layer_stack
        .with_user_config(&file, toml::from_str(initial).unwrap())
        .unwrap();
    assert!(chat.try_open_catalog_model_popup());
    chat.handle_key_event(KeyCode::Esc.into());
    let updated = format!(
        "{initial}\n[[models]]\nid = \"second\"\nname = \"Newly added model\"\nprovider = \"local\"\nmodel = \"second-wire\"\nreasoning_efforts = []\n"
    );
    std::fs::write(&file, updated).unwrap();
    assert!(chat.try_open_catalog_model_popup());
    assert!(render_bottom_popup(&chat, 100).contains("Newly added model"));
    assert_eq!(chat.current_model(), "original-model");
}

async fn mixed_provider_picker() -> (ChatWidget, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
    let (mut chat, events, _ops) = make_chatwidget_manual(Some("shared-request-model")).await;
    let mut raw = String::from(
        r#"
[model_providers.Z]
name = "Zulu Service"
base_url = "http://127.0.0.1:1/v1"
wire_api = "responses"
experimental_bearer_token = "xdcode-picker-test-credential"
[model_providers.a]
name = "ALPHA Service"
base_url = "http://127.0.0.1:2/v1"
wire_api = "responses"
experimental_bearer_token = "xdcode-picker-test-credential"
[model_providers.b]
name = "alpha service"
base_url = "http://127.0.0.1:3/v1"
wire_api = "responses"
experimental_bearer_token = "xdcode-picker-test-credential"
"#,
    );
    for (id, provider) in [
        ("z-first", "Z"),
        ("a-second", "a"),
        ("b-only", "b"),
        ("a-first", "a"),
        ("z-second", "Z"),
    ] {
        raw.push_str(&format!(
            "\n[[models]]\nid = {id:?}\nname = \"Shared display name\"\nprovider = {provider:?}\nmodel = \"shared-request-model\"\nreasoning_efforts = []\n"
        ));
    }
    let path =
        xdcode_utils_absolute_path::AbsolutePathBuf::from_absolute_path(chat.catalog_config_path())
            .unwrap();
    std::fs::write(&path, &raw).unwrap();
    chat.config.config_layer_stack = chat
        .config
        .config_layer_stack
        .with_user_config(&path, toml::from_str(&raw).unwrap())
        .unwrap();
    chat.config.model_provider_id = "b".into();
    chat.sync_catalog_identity(Some("b-only".into()));
    (chat, events)
}

#[tokio::test]
async fn catalog_groups_providers_case_insensitively_and_preserves_model_order() {
    let (mut chat, _events) = mixed_provider_picker().await;
    let before = std::fs::read_to_string(chat.catalog_config_path()).unwrap();
    assert!(chat.try_open_catalog_model_popup());
    for character in "shared".chars() {
        chat.handle_key_event(KeyCode::Char(character).into());
    }
    let rendered = render_bottom_popup(&chat, 120);
    let positions: Vec<_> = ["a-second", "a-first", "b-only", "z-first", "z-second"]
        .iter()
        .map(|id| rendered.find(&format!("[{id}]")).expect("visible model"))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{rendered}"
    );
    assert_eq!(
        std::fs::read_to_string(chat.catalog_config_path()).unwrap(),
        before
    );
    assert_eq!(chat.current_catalog_id(), Some("b-only"));
}

#[tokio::test]
async fn catalog_initial_focus_keeps_exact_id_for_duplicate_wire_models() {
    let (mut chat, mut events) = mixed_provider_picker().await;
    assert!(chat.try_open_catalog_model_popup());
    chat.handle_key_event(KeyCode::Enter.into());
    let selected = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
        AppEvent::OpenCatalogReasoning { id } => Some(id),
        _ => None,
    });
    assert_eq!(selected.as_deref(), Some("b-only"));
    assert_eq!(chat.config.model_provider_id, "b");
}

#[tokio::test]
async fn catalog_search_keeps_exact_current_id_for_duplicate_wire_models() {
    let (mut chat, mut events) = mixed_provider_picker().await;
    chat.config.model_provider_id = "a".into();
    chat.sync_catalog_identity(Some("a-second".into()));
    let before = std::fs::read_to_string(chat.catalog_config_path()).unwrap();
    assert!(chat.try_open_catalog_model_popup());
    for character in "shared".chars() {
        chat.handle_key_event(KeyCode::Char(character).into());
    }
    chat.handle_key_event(KeyCode::Enter.into());
    let selected = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
        AppEvent::OpenCatalogReasoning { id } => Some(id),
        _ => None,
    });
    assert_eq!(selected.as_deref(), Some("a-second"));
    assert_eq!(chat.current_catalog_id(), Some("a-second"));
    assert_eq!(chat.config.model_provider_id, "a");
    assert_eq!(
        std::fs::read_to_string(chat.catalog_config_path()).unwrap(),
        before
    );
}

#[tokio::test]
async fn catalog_search_matches_provider_labels_and_provider_filter_keeps_order() {
    let (mut chat, mut events) = mixed_provider_picker().await;
    assert!(chat.try_open_catalog_model_popup());
    for character in "alpha service".chars() {
        chat.handle_key_event(KeyCode::Char(character).into());
    }
    let rendered = render_bottom_popup(&chat, 120);
    assert!(
        rendered.contains("Provider: ALPHA Service (a)"),
        "{rendered}"
    );
    assert!(rendered.contains("[a-second]"), "{rendered}");
    assert!(rendered.contains("[b-only]"), "{rendered}");
    assert!(!rendered.contains("[z-first]"), "{rendered}");
    chat.handle_key_event(KeyCode::Esc.into());
    chat.open_catalog_provider("a".into());
    chat.handle_key_event(KeyCode::Enter.into());
    let selected = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
        AppEvent::OpenCatalogReasoning { id } => Some(id),
        _ => None,
    });
    assert_eq!(selected.as_deref(), Some("a-second"));
}

#[tokio::test]
async fn remembered_service_default_wins_over_a_model_default() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(None).await;
    let path = crate::project_model_memory::memory_path(
        chat.config.xdcode_home.as_path(),
        &chat.catalog_config_path(),
        chat.config.cwd.as_path(),
    );
    let mut memory = crate::project_model_memory::ProjectModelMemory::default();
    memory.last_efforts.insert("with-default".into(), None);
    memory.save(&path).unwrap();
    let mut entry = model(
        "with-default",
        "provider",
        vec![ReasoningEffortConfig::High],
    );
    entry.default_reasoning_effort = Some(ReasoningEffortConfig::High);
    chat.open_catalog_reasoning(entry);
    chat.handle_key_event(KeyCode::Enter.into());
    let selection = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| {
            if let AppEvent::SelectCatalogModel { selection, .. } = event {
                Some(selection)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(selection.effort, None);
}

#[tokio::test]
async fn first_plan_inherits_current_launch_model_instead_of_saved_normal() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some("launch-model")).await;
    let provider = chat.config.model_provider_id.clone();
    let file = chat.config.xdcode_home.join("config.toml");
    let raw = format!(
        r#"
[[models]]
id = "launch"
name = "Launch"
provider = "{provider}"
model = "launch-model"
reasoning_efforts = []
[[models]]
id = "saved"
name = "Saved"
provider = "{provider}"
model = "saved-model"
reasoning_efforts = []
"#
    );
    std::fs::write(file, raw).unwrap();
    let path = crate::project_model_memory::memory_path(
        chat.config.xdcode_home.as_path(),
        &chat.catalog_config_path(),
        chat.config.cwd.as_path(),
    );
    let mut memory = crate::project_model_memory::ProjectModelMemory::default();
    memory.remember(CatalogModelSelection {
        provider,
        catalog_id: "saved".into(),
        model: "saved-model".into(),
        effort: None,
        mode: ModeKind::Default,
    });
    memory.save(&path).unwrap();
    let plan = collaboration_modes::plan_mask(chat.model_catalog.as_ref()).unwrap();
    chat.set_collaboration_mask_from_user_action(plan);
    let selection = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| {
            if let AppEvent::SwitchCatalogMode { selection } = event {
                Some(selection)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(selection.model, "launch-model");
    assert_eq!(selection.mode, ModeKind::Plan);
    assert_eq!(chat.current_model(), "launch-model");
}

fn write_duplicate_catalog(chat: &ChatWidget) {
    let provider = &chat.config.model_provider_id;
    std::fs::write(
        chat.config.xdcode_home.join("config.toml"),
        format!(
            r#"
[[models]]
id = "first"
name = "Same model"
provider = "{provider}"
model = "shared-request-model"
reasoning_efforts = ["low"]
[[models]]
id = "second"
name = "Same model"
provider = "{provider}"
model = "shared-request-model"
reasoning_efforts = ["vendor-level"]
"#
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn duplicate_wire_models_keep_exact_catalog_id_when_switching_modes() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some("shared-request-model")).await;
    write_duplicate_catalog(&chat);
    chat.apply_confirmed_catalog_selection(&CatalogModelSelection {
        provider: chat.config.model_provider_id.clone(),
        catalog_id: "second".into(),
        model: "shared-request-model".into(),
        effort: Some(ReasoningEffortConfig::Custom("vendor-level".into())),
        mode: ModeKind::Default,
    });
    let plan = collaboration_modes::plan_mask(chat.model_catalog.as_ref()).unwrap();
    chat.set_collaboration_mask_from_user_action(plan);
    let selection = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| {
            if let AppEvent::SwitchCatalogMode { selection } = event {
                Some(selection)
            } else {
                None
            }
        })
        .expect("exact catalog choice");
    assert_eq!(selection.catalog_id, "second");
    assert_eq!(
        selection.effort,
        Some(ReasoningEffortConfig::Custom("vendor-level".into()))
    );
    assert_eq!(selection.mode, ModeKind::Plan);
}

#[tokio::test]
async fn ambiguous_legacy_wire_model_requires_explicit_catalog_choice() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some("shared-request-model")).await;
    write_duplicate_catalog(&chat);
    let before = chat.effective_collaboration_mode();
    let plan = collaboration_modes::plan_mask(chat.model_catalog.as_ref()).unwrap();
    chat.set_collaboration_mask_from_user_action(plan);
    assert_eq!(chat.effective_collaboration_mode(), before);
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::SelectCatalogModel { .. } | AppEvent::SwitchCatalogMode { .. }
        ))
    );
}

#[tokio::test]
async fn first_plan_inherits_launch_model_outside_directory_and_ignores_unsupported_override() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("custom-legacy-model")).await;
    write_duplicate_catalog(&chat);
    chat.config.plan_mode_reasoning_effort = Some(ReasoningEffortConfig::Custom(
        "different-model-level".into(),
    ));
    let effort = chat.current_reasoning_effort();
    let plan = collaboration_modes::plan_mask(chat.model_catalog.as_ref()).unwrap();
    chat.set_collaboration_mask_from_user_action(plan);
    assert_eq!(chat.current_model(), "custom-legacy-model");
    assert_eq!(chat.current_reasoning_effort(), effort);
    assert_eq!(chat.effective_collaboration_mode().mode, ModeKind::Plan);
}

#[tokio::test]
async fn restored_provider_identity_refreshes_its_distinct_base_url() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(None).await;
    std::fs::write(
        chat.config.xdcode_home.join("config.toml"),
        r#"
[model_providers.provider_a]
name = "Provider A"
base_url = "http://127.0.0.1:18801/v1"
wire_api = "responses"
[model_providers.provider_b]
name = "Provider B"
base_url = "http://127.0.0.1:18802/vendor-api"
wire_api = "responses"
"#,
    )
    .unwrap();
    chat.config.model_provider_id = "provider_b".into();
    chat.sync_catalog_identity(Some("restored-id".into()));
    assert_eq!(chat.current_catalog_id(), Some("restored-id"));
    assert_eq!(
        chat.config.model_provider.base_url.as_deref(),
        Some("http://127.0.0.1:18802/vendor-api")
    );
}

#[tokio::test]
async fn removing_directory_keeps_catalog_choice_and_blocks_legacy_fallback() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some("shared-request-model")).await;
    write_duplicate_catalog(&chat);
    chat.apply_confirmed_catalog_selection(&CatalogModelSelection {
        provider: chat.config.model_provider_id.clone(),
        catalog_id: "second".into(),
        model: "shared-request-model".into(),
        effort: None,
        mode: ModeKind::Default,
    });
    std::fs::write(chat.config.xdcode_home.join("config.toml"), "").unwrap();
    let before = chat.effective_collaboration_mode();
    // A handled catalog popup must not fall through to the mutating legacy picker.
    assert!(chat.try_open_catalog_model_popup());
    let plan = collaboration_modes::plan_mask(chat.model_catalog.as_ref()).unwrap();
    chat.set_collaboration_mask_from_user_action(plan);
    assert_eq!(chat.effective_collaboration_mode(), before);
    assert_eq!(chat.current_catalog_id(), Some("second"));
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::SelectCatalogModel { .. }
                | AppEvent::UpdateModel(_)
                | AppEvent::FetchModels { .. }
        ))
    );
}

#[tokio::test]
async fn stale_legacy_picker_actions_cannot_change_or_persist_catalog_selection() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("shared-request-model")).await;
    chat.sync_catalog_identity(Some("selected-id".into()));
    let before = chat.effective_collaboration_mode();
    for event in [
        AppEvent::UpdateModel("stale-model".into()),
        AppEvent::UpdateReasoningEffort(Some(ReasoningEffortConfig::High)),
        AppEvent::PersistModelSelection {
            model: "stale-model".into(),
            effort: None,
        },
        AppEvent::SelectSessionModel {
            model: "stale-model".into(),
            effort: None,
            service_tier: None,
        },
        AppEvent::ApplyAdvancedReasoning {
            model: "stale-model".into(),
            effort: ReasoningEffortConfig::Ultra,
        },
    ] {
        assert!(chat.reject_legacy_model_event(&event));
        assert_eq!(chat.effective_collaboration_mode(), before);
        assert_eq!(chat.current_catalog_id(), Some("selected-id"));
    }
    assert!(
        !chat.reject_legacy_model_event(&AppEvent::OpenCatalogReasoning {
            id: "selected-id".into()
        })
    );
}

#[tokio::test]
async fn removing_active_id_never_remaps_to_remaining_same_wire_entry() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(Some("shared-request-model")).await;
    write_duplicate_catalog(&chat);
    chat.apply_confirmed_catalog_selection(&CatalogModelSelection {
        provider: chat.config.model_provider_id.clone(),
        catalog_id: "second".into(),
        model: "shared-request-model".into(),
        effort: None,
        mode: ModeKind::Default,
    });
    let path = chat.config.xdcode_home.join("config.toml");
    let raw = std::fs::read_to_string(&path).unwrap();
    let second = raw.find("[[models]]\nid = \"second\"").unwrap();
    std::fs::write(path, &raw[..second]).unwrap();
    let before = chat.effective_collaboration_mode();
    let plan = collaboration_modes::plan_mask(chat.model_catalog.as_ref()).unwrap();
    chat.set_collaboration_mask_from_user_action(plan);
    assert_eq!(chat.effective_collaboration_mode(), before);
    assert_eq!(chat.current_catalog_id(), Some("second"));
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::SelectCatalogModel { .. } | AppEvent::SwitchCatalogMode { .. }
        ))
    );
}
