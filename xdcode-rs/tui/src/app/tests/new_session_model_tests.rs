use super::*;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use xdcode_protocol::protocol::CatalogModelSelection;

struct ModelFixture {
    app: Box<App>,
    events: tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    server: AppServerSession,
    requests: RecordedRequests,
    proxy: JoinHandle<Result<()>>,
    workspace: tempfile::TempDir,
    alpha: wiremock::MockServer,
    beta: wiremock::MockServer,
    config_contents: String,
}

impl ModelFixture {
    async fn new(capabilities: HistoryCapabilities, git: bool) -> Result<Self> {
        let (mut app, events, _) = make_test_app_with_channels().await;
        let workspace = tempdir()?;
        let cwd = workspace.path().join("project");
        std::fs::create_dir_all(&cwd)?;
        if git {
            std::fs::write(cwd.join("README.md"), "test project\n")?;
            for args in [
                vec!["init", "--quiet"],
                vec!["add", "."],
                vec!["commit", "--quiet", "--no-gpg-sign", "-m", "fixture"],
            ] {
                let result = std::process::Command::new("git")
                    .current_dir(&cwd)
                    .args([
                        "-c",
                        "user.name=Model Test",
                        "-c",
                        "user.email=test@example.invalid",
                    ])
                    .args(args)
                    .output()?;
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
        let alpha = responses::start_mock_server().await;
        let beta = responses::start_mock_server().await;
        let config_contents = format!(
            r#"model = "shared-model"
model_provider = "alpha"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
[features]
worktrees = true
[desktop]
git-worktree-root = {}
[projects.{}]
trust_level = "trusted"
[model_providers.alpha]
name = "Alpha"
base_url = "{}/v1"
wire_api = "responses"
experimental_bearer_token = "alpha-model-fixture"
[model_providers.beta]
name = "Beta"
base_url = "{}/v1"
wire_api = "responses"
experimental_bearer_token = "beta-model-fixture"
[[models]]
id = "alpha-exact"
name = "Shared name"
provider = "alpha"
model = "shared-model"
reasoning_efforts = ["low", "vendor-level"]
[[models]]
id = "beta-duplicate"
name = "Shared name"
provider = "beta"
model = "shared-model"
reasoning_efforts = ["vendor-level"]
[[models]]
id = "beta-exact"
name = "Shared name"
provider = "beta"
model = "shared-model"
reasoning_efforts = ["vendor-level"]
[[models]]
id = "beta-default"
name = "Shared name"
provider = "beta"
model = "shared-model"
reasoning_efforts = []
"#,
            toml::Value::String(workspace.path().join("pool").display().to_string()),
            toml::Value::String(cwd.display().to_string()),
            alpha.uri(),
            beta.uri()
        );
        std::fs::write(app.config.xdcode_home.join("config.toml"), &config_contents)?;
        app.config = app.rebuild_config_for_cwd(cwd).await?;
        app.chat_widget.sync_plugin_mentions_config(&app.config);
        app.local_settings = crate::local_settings::LocalSettings::from(&app.config);
        let (mut server, requests, proxy) = start_recording_app_server_with_history(
            &app.config,
            capabilities,
            None,
            None,
            crate::app_server_session::ThreadParamsMode::Embedded,
            LoaderOverrides::without_managed_config_for_tests(),
        )
        .await?;
        let started = server.start_thread(&app.config).await?;
        app.enqueue_primary_thread_session(started.session, started.turns)
            .await?;
        Ok(Self {
            app,
            events,
            server,
            requests,
            proxy,
            workspace,
            alpha,
            beta,
            config_contents,
        })
    }

    async fn choose(
        &mut self,
        provider: &str,
        catalog_id: &str,
        effort: Option<ReasoningEffortConfig>,
        mode: ModeKind,
    ) {
        assert!(
            self.app
                .select_catalog_model(
                    &mut self.server,
                    CatalogModelSelection {
                        provider: provider.into(),
                        catalog_id: catalog_id.into(),
                        model: "shared-model".into(),
                        effort,
                        mode,
                    },
                    true
                )
                .await
        );
    }

    async fn send_turn(
        &mut self,
        provider: &str,
        response_id: &str,
    ) -> Result<responses::ResponsesRequest> {
        let model = if provider == "alpha" {
            &self.alpha
        } else {
            &self.beta
        };
        let mock = responses::mount_sse_once(
            model,
            responses::sse(vec![
                responses::ev_response_created(response_id),
                responses::ev_assistant_message("message", "done"),
                responses::ev_completed(response_id),
            ]),
        )
        .await;
        let thread_id = self.app.chat_widget.thread_id().expect("active thread");
        let _: xdcode_app_server_protocol::TurnStartResponse = self
            .server
            .request_handle()
            .request_typed(ClientRequest::TurnStart {
                request_id: self.server.next_request_id(),
                params: serde_json::from_value(serde_json::json!({
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": "Reply briefly", "text_elements": []}],
                }))?,
            })
            .await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(event) = self.server.next_event().await {
                if let AppServerEvent::ServerNotification(notification) = event.event
                    && let ServerNotification::TurnCompleted(completed) = *notification
                    && completed.thread_id == thread_id.to_string()
                {
                    assert_eq!(
                        completed.turn.status,
                        xdcode_app_server_protocol::TurnStatus::Completed
                    );
                    return;
                }
            }
            panic!("app server closed before turn completion");
        })
        .await?;
        Ok(mock.single_request())
    }

    fn assert_config_unchanged(&self) -> Result<()> {
        assert_eq!(
            std::fs::read_to_string(self.app.config.xdcode_home.join("config.toml"))?,
            self.config_contents
        );
        assert!(recorded_params(&self.requests, "config/batchWrite").is_empty());
        Ok(())
    }

    async fn shutdown(self) -> Result<()> {
        self.server.shutdown().await?;
        self.proxy.await??;
        Ok(())
    }
}

#[tokio::test]
async fn new_inherits_confirmed_catalog_routing_effort_and_mode_before_first_turn() -> Result<()> {
    for (mode, effort, name) in [
        (
            ModeKind::Default,
            Some(ReasoningEffortConfig::Custom("vendor-level".into())),
            None,
        ),
        (
            ModeKind::Plan,
            Some(ReasoningEffortConfig::Custom("vendor-level".into())),
            Some("named task"),
        ),
        (ModeKind::Default, None, Some("service default")),
        (ModeKind::Plan, None, None),
    ] {
        let mut fixture = ModelFixture::new(HistoryCapabilities::ModelSelection, false).await?;
        let old_request = fixture.send_turn("alpha", "old-alpha").await?;
        assert_eq!(
            old_request.header("authorization").as_deref(),
            Some("Bearer alpha-model-fixture")
        );
        assert_eq!(old_request.body_json()["model"], "shared-model");
        let other_mode = if mode == ModeKind::Default {
            ModeKind::Plan
        } else {
            ModeKind::Default
        };
        fixture
            .choose(
                "alpha",
                "alpha-exact",
                Some(ReasoningEffortConfig::Low),
                other_mode,
            )
            .await;
        let catalog_id = if effort.is_some() {
            "beta-exact"
        } else {
            "beta-default"
        };
        fixture
            .choose("beta", catalog_id, effort.clone(), mode)
            .await;
        let confirmed = fixture.app.chat_widget.confirmed_model_selection().unwrap();
        let old_thread = fixture.app.chat_widget.thread_id().unwrap();
        let memory_path = crate::project_model_memory::memory_path(
            fixture.app.config.xdcode_home.as_path(),
            &fixture.app.chat_widget.catalog_config_path(),
            fixture.app.chat_widget.config_ref().cwd.as_path(),
        );
        let memory_before = std::fs::read(&memory_path)?;
        fixture.server.model_provider_override = Some("alpha".into());
        fixture.app.harness_overrides.model = Some("stale-startup-model".into());
        fixture
            .app
            .cli_kv_overrides
            .push(("model_reasoning_effort".into(), "high".into()));
        fixture.app.chat_widget.set_model("unconfirmed-model");
        fixture
            .app
            .chat_widget
            .set_reasoning_effort(Some(ReasoningEffortConfig::High));
        fixture
            .app
            .chat_widget
            .set_collaboration_mask(CollaborationModeMask {
                name: "Unconfirmed mode".into(),
                mode: Some(other_mode),
                model: Some("unconfirmed-model".into()),
                reasoning_effort: Some(Some(ReasoningEffortConfig::High)),
                developer_instructions: Some(None),
            });
        assert_eq!(
            fixture.app.confirmed_new_session_selection().await?,
            Some(confirmed.clone())
        );
        while fixture.events.try_recv().is_ok() {}
        let command = name.map_or("/new".into(), |name| format!("/new {name}"));
        fixture.app.chat_widget.apply_external_edit(command.clone());
        fixture
            .app
            .chat_widget
            .handle_key_event(KeyCode::Enter.into());
        let event = std::iter::from_fn(|| fixture.events.try_recv().ok())
            .find(|event| matches!(event, AppEvent::NewSession { .. }))
            .expect("new command");
        assert_eq!(
            fixture.app.chat_widget.composer_text_with_pending(),
            command
        );
        let mut tui = crate::tui::test_support::make_test_tui()?;
        fixture
            .app
            .handle_event(&mut tui, &mut fixture.server, event)
            .await?;
        let new_thread = fixture.app.chat_widget.thread_id().unwrap();
        assert_ne!(new_thread, old_thread);
        assert_eq!(
            fixture.app.chat_widget.confirmed_model_selection(),
            Some(confirmed)
        );
        assert_eq!(
            fixture.app.chat_widget.current_catalog_id(),
            Some(catalog_id)
        );
        assert_eq!(fixture.app.chat_widget.composer_text_with_pending(), "");
        assert_eq!(std::fs::read(&memory_path)?, memory_before);
        let starts = recorded_params(&fixture.requests, "thread/start");
        let new_start = starts.last().unwrap();
        assert_eq!(new_start["modelProvider"], "beta");
        assert_eq!(new_start["model"], "shared-model");
        let updates = recorded_params(&fixture.requests, "thread/settings/update");
        let confirmed_update = updates.last().unwrap();
        assert_eq!(confirmed_update["threadId"], new_thread.to_string());
        assert_eq!(
            confirmed_update["catalogSelection"]["catalogId"],
            catalog_id
        );
        let request = fixture.send_turn("beta", "new-beta").await?;
        assert_eq!(
            request.header("authorization").as_deref(),
            Some("Bearer beta-model-fixture")
        );
        assert_eq!(request.body_json()["model"], "shared-model");
        assert_eq!(
            request.body_json()["reasoning"]["effort"],
            serde_json::to_value(effort)?
        );
        assert_eq!(
            request
                .body_json()
                .to_string()
                .contains("Plan Mode (Conversational)"),
            mode == ModeKind::Plan
        );
        let turns = recorded_params(&fixture.requests, "turn/start");
        assert!(turns.last().unwrap()["model"].is_null());
        assert!(turns.last().unwrap()["effort"].is_null());
        fixture.assert_config_unchanged()?;
        fixture.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn new_settings_rejection_preserves_old_session_command_and_attachments() -> Result<()> {
    for (capabilities, command) in [
        (
            HistoryCapabilities::NewThreadSettingsRejected,
            "/new preserved task",
        ),
        (
            HistoryCapabilities::NewThreadSettingsUnsupported,
            "/new preserved task",
        ),
        (HistoryCapabilities::NewThreadSettingsRejected, "/new "),
        (HistoryCapabilities::NewThreadSettingsUnsupported, "/new "),
    ] {
        let mut fixture = ModelFixture::new(capabilities, false).await?;
        fixture
            .choose(
                "beta",
                "beta-exact",
                Some(ReasoningEffortConfig::Custom("vendor-level".into())),
                ModeKind::Plan,
            )
            .await;
        let old_thread = fixture.app.chat_widget.thread_id().unwrap();
        let confirmed = fixture.app.chat_widget.confirmed_model_selection();
        let config_before = fixture.app.config.clone();
        let image_path = fixture.workspace.path().join("draft.png");
        image::RgbaImage::new(1, 1).save(&image_path)?;
        fixture.app.chat_widget.apply_external_edit(command.into());
        fixture
            .app
            .chat_widget
            .set_remote_image_urls(vec!["https://example.invalid/draft.png".into()]);
        fixture.app.chat_widget.attach_image(image_path);
        let input_before = fixture
            .app
            .chat_widget
            .capture_thread_input_state()
            .unwrap();
        while fixture.events.try_recv().is_ok() {}
        fixture
            .app
            .chat_widget
            .handle_key_event(KeyCode::Enter.into());
        let event = std::iter::from_fn(|| fixture.events.try_recv().ok())
            .find(|event| matches!(event, AppEvent::NewSession { .. }))
            .expect("new command");
        let mut tui = crate::tui::test_support::make_test_tui()?;
        fixture
            .app
            .handle_event(&mut tui, &mut fixture.server, event)
            .await?;
        assert_eq!(fixture.app.chat_widget.thread_id(), Some(old_thread));
        assert_eq!(fixture.app.active_thread_id, Some(old_thread));
        assert_eq!(
            fixture.app.chat_widget.confirmed_model_selection(),
            confirmed
        );
        assert_eq!(fixture.app.config, config_before);
        assert_eq!(
            fixture
                .app
                .chat_widget
                .capture_thread_input_state()
                .unwrap(),
            input_before
        );
        assert!(recorded_params(&fixture.requests, "turn/start").is_empty());
        let archives = recorded_params(&fixture.requests, "thread/archive");
        assert_eq!(archives.len(), 1);
        assert_ne!(archives[0]["threadId"], old_thread.to_string());
        let unsubscribed = recorded_params(&fixture.requests, "thread/unsubscribe");
        assert!(
            unsubscribed
                .iter()
                .all(|params| params["threadId"] != old_thread.to_string())
        );
        fixture.assert_config_unchanged()?;
        fixture.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn new_without_catalog_confirms_explicit_service_default_and_plan_mode() -> Result<()> {
    let mut fixture = ModelFixture::new(HistoryCapabilities::ModelSelection, false).await?;
    let thread_id = fixture.app.chat_widget.thread_id().unwrap();
    let mode = CollaborationMode {
        mode: ModeKind::Plan,
        settings: xdcode_protocol::config_types::Settings {
            model: "shared-model".into(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    assert!(
        fixture
            .server
            .thread_settings_update(xdcode_app_server_protocol::ThreadSettingsUpdateParams {
                thread_id: thread_id.to_string(),
                model: Some("shared-model".into()),
                effort: Some(None),
                collaboration_mode: Some(mode),
                ..Default::default()
            })
            .await?
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = fixture.server.next_event().await {
            let accepted = matches!(&event.event,
                AppServerEvent::ServerNotification(notification)
                    if matches!(notification.as_ref(), ServerNotification::ThreadSettingsUpdated(updated)
                        if updated.thread_id == thread_id.to_string()
                            && updated.thread_settings.collaboration_mode.mode == ModeKind::Plan
                            && updated.thread_settings.effort.is_none()));
            fixture.app.handle_app_server_event(&fixture.server, event.event).await;
            if accepted { return; }
        }
        panic!("missing confirmed settings");
    }).await?;
    let confirmed = fixture.app.chat_widget.confirmed_model_selection().unwrap();
    assert!(confirmed.catalog_id.is_none());
    fixture.server.model_provider_override = Some("beta".into());
    let mut tui = crate::tui::test_support::make_test_tui()?;
    fixture
        .app
        .start_fresh_session(&mut tui, &mut fixture.server, None, None, None)
        .await;
    assert_ne!(fixture.app.chat_widget.thread_id(), Some(thread_id));
    assert_eq!(
        fixture.app.chat_widget.confirmed_model_selection(),
        Some(confirmed)
    );
    let updates = recorded_params(&fixture.requests, "thread/settings/update");
    assert_eq!(updates.last().unwrap()["effort"], serde_json::Value::Null);
    assert!(
        updates
            .last()
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("effort")
    );
    assert!(updates.last().unwrap()["catalogSelection"].is_null());
    let request = fixture.send_turn("alpha", "uncatalogued-alpha").await?;
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer alpha-model-fixture")
    );
    assert!(request.body_json()["reasoning"]["effort"].is_null());
    assert!(
        request
            .body_json()
            .to_string()
            .contains("Plan Mode (Conversational)")
    );
    fixture.assert_config_unchanged()?;
    fixture.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn clear_retains_launch_model_priority_instead_of_new_inheritance() -> Result<()> {
    let mut fixture = ModelFixture::new(HistoryCapabilities::ModelSelection, false).await?;
    fixture
        .choose("beta", "beta-default", None, ModeKind::Plan)
        .await;
    fixture.server.model_provider_override = Some("alpha".into());
    fixture.app.harness_overrides.model = Some("explicit-launch-model".into());
    let updates_before = recorded_params(&fixture.requests, "thread/settings/update").len();
    let mut tui = crate::tui::test_support::make_test_tui()?;
    fixture
        .app
        .start_fresh_session(
            &mut tui,
            &mut fixture.server,
            Some(ThreadStartSource::Clear),
            None,
            None,
        )
        .await;
    assert_eq!(
        fixture.app.chat_widget.current_model(),
        "explicit-launch-model"
    );
    let starts = recorded_params(&fixture.requests, "thread/start");
    assert_eq!(starts.last().unwrap()["modelProvider"], "alpha");
    assert_eq!(
        recorded_params(&fixture.requests, "thread/settings/update").len(),
        updates_before
    );
    fixture.assert_config_unchanged()?;
    fixture.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn new_worktree_inherits_model_without_copying_project_preferences() -> Result<()> {
    let mut fixture = ModelFixture::new(HistoryCapabilities::ModelSelection, true).await?;
    fixture
        .choose(
            "alpha",
            "alpha-exact",
            Some(ReasoningEffortConfig::Low),
            ModeKind::Default,
        )
        .await;
    fixture
        .choose("beta", "beta-default", None, ModeKind::Plan)
        .await;
    let confirmed = fixture.app.chat_widget.confirmed_model_selection();
    let source = fixture.app.config.cwd.clone();
    let old_thread = fixture.app.chat_widget.thread_id().unwrap();
    let old_memory = crate::project_model_memory::memory_path(
        fixture.app.config.xdcode_home.as_path(),
        &fixture.app.chat_widget.catalog_config_path(),
        source.as_path(),
    );
    let old_contents = std::fs::read(&old_memory)?;
    fixture.server.model_provider_override = Some("alpha".into());
    let mut tui = crate::tui::test_support::make_test_tui()?;
    fixture
        .app
        .handle_event(
            &mut tui,
            &mut fixture.server,
            AppEvent::StartManagedWorktree {
                mode: crate::app_event::ManagedWorktreeMode::New,
                name: Some("inherited task".into()),
            },
        )
        .await?;
    complete_managed_worktree_creation(
        &mut fixture.app,
        &mut tui,
        &mut fixture.server,
        &mut fixture.events,
    )
    .await?;
    assert_ne!(fixture.app.chat_widget.thread_id(), Some(old_thread));
    assert_ne!(fixture.app.config.cwd, source);
    assert_eq!(
        fixture.app.chat_widget.confirmed_model_selection(),
        confirmed
    );
    let new_memory = crate::project_model_memory::memory_path(
        fixture.app.config.xdcode_home.as_path(),
        &fixture.app.chat_widget.catalog_config_path(),
        fixture.app.config.cwd.as_path(),
    );
    assert_ne!(old_memory, new_memory);
    assert!(!new_memory.exists());
    assert_eq!(std::fs::read(old_memory)?, old_contents);
    let request = fixture.send_turn("beta", "worktree-beta").await?;
    assert_eq!(
        request.header("authorization").as_deref(),
        Some("Bearer beta-model-fixture")
    );
    assert!(request.body_json()["reasoning"]["effort"].is_null());
    assert!(
        request
            .body_json()
            .to_string()
            .contains("Plan Mode (Conversational)")
    );
    fixture.assert_config_unchanged()?;
    fixture.shutdown().await?;
    Ok(())
}
