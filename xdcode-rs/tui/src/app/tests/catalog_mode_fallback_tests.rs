//! Mode fallback is a settings-only retry, with no user turn or preference write before acceptance.
use super::*;
use pretty_assertions::assert_eq;
use xdcode_app_server_client::AppServerClient;
use xdcode_app_server_client::CloudClient;
use xdcode_app_server_client::CloudCommand;
use xdcode_app_server_client::CloudOperation;
use xdcode_app_server_protocol::ClientRequest;
use xdcode_app_server_protocol::JSONRPCErrorError;
use xdcode_protocol::protocol::CatalogModelSelection;

struct Fixture {
    app: Box<App>,
    events: tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ops: tokio::sync::mpsc::UnboundedReceiver<Op>,
    memory_path: std::path::PathBuf,
    current: CatalogModelSelection,
    remembered: CatalogModelSelection,
}

impl Fixture {
    async fn new(target: ModeKind, effort: Option<ReasoningEffortConfig>) -> Result<Self> {
        let (mut app, events, mut ops) = make_test_app_with_channels().await;
        let file = app.config.xdcode_home.join("config.toml");
        let raw = r#"
[model_providers.current]
name = "Current provider"
base_url = "http://127.0.0.1:1/v1"
wire_api = "responses"
experimental_bearer_token = "xdcode-mode-fixture-credential"
[model_providers.other]
name = "Other provider"
base_url = "http://127.0.0.1:2/v1"
wire_api = "responses"
experimental_bearer_token = "xdcode-mode-fixture-credential"
[[models]]
id = "current-duplicate"
name = "current-duplicate"
provider = "current"
model = "same-wire-model"
reasoning_efforts = ["none", "vendor-level", "max"]
[[models]]
id = "current-exact"
name = "current-exact"
provider = "current"
model = "same-wire-model"
reasoning_efforts = ["none", "vendor-level", "max"]
[[models]]
id = "remembered"
name = "remembered"
provider = "other"
model = "remembered-wire-model"
reasoning_efforts = ["max"]
"#;
        std::fs::write(&file, raw)?;
        app.config.config_layer_stack = app
            .config
            .config_layer_stack
            .with_user_config(&file, toml::from_str(raw)?)?;
        app.chat_widget.sync_plugin_mentions_config(&app.config);
        let thread_id = ThreadId::new();
        app.active_thread_id = Some(thread_id);
        app.primary_thread_id = Some(thread_id);
        let session = test_thread_session(thread_id, app.config.cwd.to_path_buf());
        app.primary_session_configured = Some(session.clone());
        app.chat_widget.handle_thread_session(session);
        let current = CatalogModelSelection {
            provider: "current".into(),
            catalog_id: "current-exact".into(),
            model: "same-wire-model".into(),
            effort,
            mode: match target {
                ModeKind::Default => ModeKind::Plan,
                ModeKind::Plan => ModeKind::Default,
            },
        };
        let remembered = CatalogModelSelection {
            provider: "other".into(),
            catalog_id: "remembered".into(),
            model: "remembered-wire-model".into(),
            effort: Some(ReasoningEffortConfig::Max),
            mode: target,
        };
        app.chat_widget.apply_confirmed_catalog_selection(&current);
        let memory_path = crate::project_model_memory::memory_path(
            app.config.xdcode_home.as_path(),
            &app.chat_widget.catalog_config_path(),
            app.chat_widget.config_ref().cwd.as_path(),
        );
        let mut memory = crate::project_model_memory::ProjectModelMemory::default();
        memory.remember(remembered.clone());
        memory.remember(current.clone());
        memory
            .save(&memory_path)
            .map_err(|error| color_eyre::eyre::eyre!(error.to_string()))?;
        while let Ok(op) = ops.try_recv() {
            assert!(!matches!(op, Op::UserTurn { .. }));
        }
        Ok(Self {
            app,
            events,
            ops,
            memory_path,
            current,
            remembered,
        })
    }

    fn error_text(&mut self) -> String {
        std::iter::from_fn(|| self.events.try_recv().ok())
            .filter_map(|event| match event {
                AppEvent::InsertHistoryCell(cell) => {
                    let visible = cell
                        .display_lines(240)
                        .iter()
                        .map(|line| {
                            line.spans
                                .iter()
                                .map(|span| span.content.as_ref())
                                .collect::<String>()
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let diagnostics = cell
                        .warning_entries()
                        .iter()
                        .map(|entry| entry.details.clone())
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(format!("{visible}\n{diagnostics}"))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn server() -> (AppServerSession, tokio::sync::mpsc::Receiver<CloudCommand>) {
    let (client, commands, _notifications) = CloudClient::channel(1);
    (
        AppServerSession::new(
            crate::backend_id::BackendId::new(),
            AppServerClient::Cloud(client),
            crate::app_server_session::ThreadParamsMode::Embedded,
        ),
        commands,
    )
}

fn selection(command: &CloudCommand) -> &CatalogModelSelection {
    let CloudOperation::Request(request) = &command.operation else {
        panic!("expected settings RPC");
    };
    let ClientRequest::ThreadSettingsUpdate { params, .. } = request.as_ref() else {
        panic!("expected settings update");
    };
    assert!(params.model.is_none());
    assert!(params.effort.is_none());
    params.catalog_selection.as_ref().expect("atomic selection")
}

fn reject_history(command: CloudCommand) {
    command
        .response
        .send(Ok(Err(JSONRPCErrorError {
            code: -32602,
            message: "provider-specific encrypted history cannot be transferred".into(),
            data: Some(serde_json::json!({"reason": "providerHistoryNotPortable"})),
        })))
        .unwrap();
}

fn accept(command: CloudCommand) {
    command
        .response
        .send(Ok(Ok(serde_json::json!({}))))
        .unwrap();
}

#[tokio::test]
async fn mode_history_fallback_persists_only_after_ack_and_preserves_exact_identity() -> Result<()>
{
    for target in [ModeKind::Plan, ModeKind::Default] {
        for effort in [
            None,
            Some(ReasoningEffortConfig::None),
            Some(ReasoningEffortConfig::Custom("vendor-level".into())),
        ] {
            let mut fixture = Fixture::new(target, effort).await?;
            let before = std::fs::read(&fixture.memory_path)?;
            let remembered = fixture
                .app
                .chat_widget
                .catalog_mode_selection(target)
                .map_err(|error| color_eyre::eyre::eyre!(error))?
                .unwrap();
            assert_eq!(remembered, fixture.remembered);
            let (mut server, mut commands) = server();
            let mut fallback = fixture.current.clone();
            fallback.mode = target;
            {
                let update = fixture.app.switch_catalog_mode(&mut server, remembered);
                tokio::pin!(update);
                assert!(futures::poll!(&mut update).is_pending());
                let first = commands.recv().await.unwrap();
                assert_eq!(selection(&first), &fixture.remembered);
                reject_history(first);
                assert!(futures::poll!(&mut update).is_pending());
                let second = commands.recv().await.unwrap();
                assert_eq!(selection(&second), &fallback);
                assert_eq!(std::fs::read(&fixture.memory_path)?, before);
                assert!(fixture.ops.try_recv().is_err());
                accept(second);
                assert!(update.await);
            }
            assert!(commands.try_recv().is_err());
            assert_eq!(
                fixture.app.chat_widget.current_catalog_id(),
                Some("current-exact")
            );
            assert_eq!(
                fixture.app.chat_widget.effective_collaboration_mode().mode,
                target
            );
            assert_eq!(
                fixture.app.chat_widget.current_reasoning_effort(),
                fallback.effort
            );
            let memory =
                crate::project_model_memory::ProjectModelMemory::load(&fixture.memory_path)
                    .map_err(|error| color_eyre::eyre::eyre!(error.to_string()))?;
            assert_eq!(memory.selection(target), Some(&fallback));
            assert_eq!(
                memory.selection(fixture.current.mode),
                Some(&fixture.current)
            );
            assert_eq!(memory.last_mode, Some(target));
            assert_eq!(
                fixture
                    .app
                    .chat_widget
                    .catalog_mode_selection(target)
                    .map_err(|error| color_eyre::eyre::eyre!(error))?,
                Some(fallback)
            );
            let cached = fixture.app.primary_session_configured.as_ref().unwrap();
            assert_eq!(cached.model_provider_id, "current");
            assert_eq!(cached.catalog_id.as_deref(), Some("current-exact"));
            assert!(fixture.error_text().contains("could not be applied"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn transport_and_untyped_server_errors_do_not_trigger_mode_fallback() -> Result<()> {
    for transport in [true, false] {
        let mut fixture = Fixture::new(ModeKind::Plan, None).await?;
        let before = std::fs::read(&fixture.memory_path)?;
        let (mut server, mut commands) = server();
        {
            let update = fixture
                .app
                .switch_catalog_mode(&mut server, fixture.remembered.clone());
            tokio::pin!(update);
            assert!(futures::poll!(&mut update).is_pending());
            let command = commands.recv().await.unwrap();
            if transport {
                command
                    .response
                    .send(Err(std::io::Error::other("transport disconnected")))
                    .unwrap();
            } else {
                command
                    .response
                    .send(Ok(Err(JSONRPCErrorError {
                        code: -32602,
                        message: "provider-specific encrypted history cannot be transferred".into(),
                        data: None,
                    })))
                    .unwrap();
            }
            assert!(!update.await);
        }
        assert!(commands.try_recv().is_err());
        assert_eq!(
            fixture.app.chat_widget.effective_collaboration_mode().mode,
            fixture.current.mode
        );
        assert_eq!(std::fs::read(&fixture.memory_path)?, before);
        let text = fixture.error_text();
        assert!(
            text.contains("thread/settings/update failed in TUI"),
            "{text}"
        );
        assert!(
            text.contains(if transport {
                "transport disconnected"
            } else {
                "encrypted history"
            }),
            "{text}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn explicit_model_picker_rejection_never_overwrites_a_mode_preference() -> Result<()> {
    let mut fixture = Fixture::new(ModeKind::Plan, None).await?;
    let before = std::fs::read(&fixture.memory_path)?;
    let (mut server, mut commands) = server();
    {
        let update =
            fixture
                .app
                .select_catalog_model(&mut server, fixture.remembered.clone(), true);
        tokio::pin!(update);
        assert!(futures::poll!(&mut update).is_pending());
        reject_history(commands.recv().await.unwrap());
        assert!(!update.await);
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(
        fixture.app.chat_widget.effective_collaboration_mode().mode,
        fixture.current.mode
    );
    assert_eq!(std::fs::read(&fixture.memory_path)?, before);
    Ok(())
}

#[tokio::test]
async fn failed_mode_fallback_preserves_draft_and_preferences_without_replaying() -> Result<()> {
    let mut fixture = Fixture::new(ModeKind::Default, None).await?;
    let before = std::fs::read(&fixture.memory_path)?;
    let mask = crate::collaboration_modes::default_mode_mask(&fixture.app.model_catalog).unwrap();
    let (mut server, mut commands) = server();
    {
        let update =
            fixture
                .app
                .submit_mode_message(&mut server, "Implement the plan.".into(), mask);
        tokio::pin!(update);
        assert!(futures::poll!(&mut update).is_pending());
        reject_history(commands.recv().await.unwrap());
        assert!(futures::poll!(&mut update).is_pending());
        reject_history(commands.recv().await.unwrap());
        update.await;
    }
    assert!(commands.try_recv().is_err(), "only one fallback is allowed");
    assert!(fixture.ops.try_recv().is_err());
    assert_eq!(
        fixture.app.chat_widget.composer_text_with_pending(),
        "Implement the plan."
    );
    assert_eq!(
        fixture.app.chat_widget.effective_collaboration_mode().mode,
        ModeKind::Plan
    );
    assert_eq!(std::fs::read(&fixture.memory_path)?, before);
    Ok(())
}

#[tokio::test]
async fn accepted_mode_fallback_submits_one_message_after_settings_are_accepted() -> Result<()> {
    let mut fixture = Fixture::new(ModeKind::Default, None).await?;
    let mask = crate::collaboration_modes::default_mode_mask(&fixture.app.model_catalog).unwrap();
    let (mut server, mut commands) = server();
    {
        let update =
            fixture
                .app
                .submit_mode_message(&mut server, "Implement the plan.".into(), mask);
        tokio::pin!(update);
        assert!(futures::poll!(&mut update).is_pending());
        reject_history(commands.recv().await.unwrap());
        assert!(futures::poll!(&mut update).is_pending());
        let fallback = commands.recv().await.unwrap();
        assert_eq!(selection(&fallback).provider, "current");
        assert!(fixture.ops.try_recv().is_err());
        accept(fallback);
        update.await;
    }
    let Op::UserTurn {
        items,
        collaboration_mode: Some(mode),
        ..
    } = fixture.ops.try_recv().expect("one turn")
    else {
        panic!("expected a mode-bound user turn");
    };
    assert_eq!(mode.mode, ModeKind::Default);
    assert_eq!(mode.model(), "same-wire-model");
    assert_eq!(
        items,
        vec![AppServerUserInput::Text {
            text: "Implement the plan.".into(),
            text_elements: Vec::new()
        }]
    );
    assert!(fixture.ops.try_recv().is_err());
    assert!(commands.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn removed_remembered_model_falls_back_to_current_catalog_entry() -> Result<()> {
    let mut fixture = Fixture::new(ModeKind::Plan, None).await?;
    let path = fixture.app.chat_widget.catalog_config_path();
    let config = std::fs::read_to_string(&path)?.replace("id = \"remembered\"", "id = \"removed\"");
    std::fs::write(path, config)?;
    let (mut server, mut commands) = server();
    {
        let update = fixture
            .app
            .switch_catalog_mode(&mut server, fixture.remembered.clone());
        tokio::pin!(update);
        assert!(futures::poll!(&mut update).is_pending());
        let fallback = commands.recv().await.unwrap();
        assert_eq!(selection(&fallback).catalog_id, "current-exact");
        accept(fallback);
        assert!(update.await);
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(
        fixture.app.chat_widget.effective_collaboration_mode().mode,
        ModeKind::Plan
    );
    Ok(())
}

#[tokio::test]
async fn fallback_save_failure_keeps_the_accepted_session_and_warns() -> Result<()> {
    let mut fixture = Fixture::new(ModeKind::Plan, None).await?;
    std::fs::remove_file(&fixture.memory_path)?;
    std::fs::create_dir(&fixture.memory_path)?;
    let (mut server, mut commands) = server();
    {
        let update = fixture
            .app
            .switch_catalog_mode(&mut server, fixture.remembered.clone());
        tokio::pin!(update);
        assert!(futures::poll!(&mut update).is_pending());
        reject_history(commands.recv().await.unwrap());
        assert!(futures::poll!(&mut update).is_pending());
        accept(commands.recv().await.unwrap());
        assert!(update.await);
    }
    assert!(fixture.memory_path.is_dir());
    assert_eq!(
        fixture.app.chat_widget.effective_collaboration_mode().mode,
        ModeKind::Plan
    );
    assert!(
        fixture
            .error_text()
            .contains("project preferences could not be saved")
    );
    Ok(())
}
