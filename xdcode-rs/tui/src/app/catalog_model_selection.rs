//! Apply catalog choices after validation, with server acknowledgment before UI or memory changes.
use super::*;
use crate::app_server_session::is_provider_history_not_portable_error;
use xdcode_app_server_protocol::ThreadSettingsUpdateParams;
use xdcode_config::model_catalog::ModelStatus;
use xdcode_config::model_catalog::static_check;
use xdcode_protocol::config_types::ModeKind;
use xdcode_protocol::protocol::CatalogModelSelection;

enum CatalogSelectionError {
    Validation(String),
    Update(color_eyre::eyre::Report),
}

impl CatalogSelectionError {
    fn allows_mode_fallback(&self) -> bool {
        match self {
            Self::Validation(_) => true,
            Self::Update(error) => is_provider_history_not_portable_error(error),
        }
    }

    fn fallback_reason(&self) -> &str {
        match self {
            Self::Validation(message) => message,
            Self::Update(_) => "the current history cannot be transferred to that provider",
        }
    }
}

impl App {
    pub(super) async fn open_catalog_reasoning(&mut self, id: String) {
        let snapshot = match self.chat_widget.user_model_catalog() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Cannot refresh model configuration: {error}"));
                return;
            }
        };
        let Ok(model) = snapshot.model(&id) else {
            self.chat_widget
                .add_error_message(format!("Model {id} is no longer configured."));
            return;
        };
        let mut model = model.clone();
        if model.reasoning_efforts.is_none() {
            self.chat_widget.add_info_message(
                "Discovering reasoning levels (up to 60 seconds)…".into(),
                None,
            );
            let stack = self.chat_widget.config_ref().config_layer_stack.clone();
            let tx = self.app_event_tx.clone();
            let thread_id = self.chat_widget.thread_id();
            let mode = self.chat_widget.effective_collaboration_mode().mode;
            tokio::spawn(async move {
                let warning = match xdcode_config::model_catalog::refresh_reasoning_layered(
                    &stack, &id, false,
                )
                .await
                {
                    Ok(result) => {
                        model = result.model;
                        result.message
                    }
                    Err(error) => Some(format!(
                        "Reasoning discovery failed: {error}. No unsupported-capability result was saved."
                    )),
                };
                tx.send(AppEvent::CatalogReasoningDiscovered {
                    thread_id,
                    mode,
                    model,
                    warning,
                });
            });
            return;
        }
        self.chat_widget.open_catalog_reasoning(model);
    }

    pub(super) async fn select_catalog_model(
        &mut self,
        app_server: &mut AppServerSession,
        selection: CatalogModelSelection,
        persist: bool,
    ) -> bool {
        match self
            .try_select_catalog_model(app_server, &selection, persist)
            .await
        {
            Ok(updated) => updated,
            Err(error) => {
                self.report_catalog_selection_error(&error);
                false
            }
        }
    }

    /// Recover an unavailable remembered mode choice once, using the current acknowledged model.
    /// Explicit picker choices never enter this path. Preferences change only after acknowledgment.
    pub(super) async fn switch_catalog_mode(
        &mut self,
        app_server: &mut AppServerSession,
        selection: CatalogModelSelection,
    ) -> bool {
        let fallback = self
            .chat_widget
            .catalog_mode_fallback_selection(selection.mode);
        let error = match self
            .try_select_catalog_model(app_server, &selection, true)
            .await
        {
            Ok(updated) => return updated,
            Err(error) => error,
        };
        let Some(fallback) =
            fallback.filter(|fallback| fallback != &selection && error.allows_mode_fallback())
        else {
            self.report_catalog_selection_error(&error);
            return false;
        };
        match self
            .try_select_catalog_model(app_server, &fallback, true)
            .await
        {
            Ok(true) => {
                self.chat_widget.add_info_message(
                    format!(
                        "The remembered {} model ({} / {}) could not be applied: {}. {} now uses {} / {}.",
                        selection.mode.display_name(), selection.provider, selection.model,
                        error.fallback_reason(), fallback.mode.display_name(), fallback.provider,
                        fallback.model,
                    ),
                    None,
                );
                true
            }
            Ok(false) => {
                self.report_catalog_selection_error(&error);
                false
            }
            Err(fallback_error) => {
                self.report_catalog_selection_error(&error);
                self.report_catalog_selection_error(&fallback_error);
                false
            }
        }
    }

    fn report_catalog_selection_error(&mut self, error: &CatalogSelectionError) {
        match error {
            CatalogSelectionError::Validation(message) => self
                .chat_widget
                .add_error_message(format!("Model selection was not changed: {message}")),
            CatalogSelectionError::Update(error) => {
                tracing::warn!("failed to update app-server model settings from TUI: {error:#}");
                self.chat_widget
                    .add_error_message(format!("Failed to update thread settings: {error:#}",));
            }
        }
    }

    async fn try_select_catalog_model(
        &mut self,
        app_server: &mut AppServerSession,
        selection: &CatalogModelSelection,
        persist: bool,
    ) -> std::result::Result<bool, CatalogSelectionError> {
        let Some(thread_id) = self.active_thread_id else {
            self.chat_widget
                .add_error_message("Wait for the session to start before choosing a model.".into());
            return Ok(false);
        };
        // Validate again: the configuration may have changed while the picker was open.
        let validation = self.chat_widget.user_model_catalog().and_then(|snapshot| {
            let model = snapshot.model(&selection.catalog_id)?;
            if model.provider != selection.provider || model.model != selection.model {
                anyhow::bail!("model definition changed; reopen /model");
            }
            if selection
                .effort
                .as_ref()
                .is_some_and(|effort| !model.supports_effort(effort))
            {
                anyhow::bail!("selected reasoning level is no longer supported; reopen /model");
            }
            let check = static_check(&snapshot, &selection.catalog_id)?;
            if matches!(
                check.status,
                ModelStatus::MissingCredentials | ModelStatus::Incompatible
            ) {
                anyhow::bail!("{}", check.issues.join("; "));
            }
            Ok(())
        });
        validation.map_err(|error| CatalogSelectionError::Validation(error.to_string()))?;
        let was_running = self.chat_widget.is_user_turn_pending_or_running();
        let updated = app_server
            .thread_settings_update(ThreadSettingsUpdateParams {
                thread_id: thread_id.to_string(),
                catalog_selection: Some(selection.clone()),
                ..ThreadSettingsUpdateParams::default()
            })
            .await
            .map_err(CatalogSelectionError::Update)?;
        if !updated {
            self.chat_widget.add_warning_message("The server did not accept the model settings update; the previous selection remains active.".into());
            return Ok(false);
        }
        // Local settings advance only after the server has accepted the complete routing choice.
        if self.active_thread_id != Some(thread_id)
            || self.chat_widget.thread_id() != Some(thread_id)
        {
            return Ok(false);
        }
        self.chat_widget
            .apply_confirmed_catalog_selection(selection);
        self.sync_confirmed_catalog_selection_to_sessions(selection)
            .await;
        if persist {
            let path = crate::project_model_memory::memory_path(
                self.config.xdcode_home.as_path(),
                &self.chat_widget.catalog_config_path(),
                self.chat_widget.config_ref().cwd.as_path(),
            );
            let saved = crate::project_model_memory::ProjectModelMemory::load(&path).and_then(
                |mut memory| {
                    memory.remember(selection.clone());
                    memory.save(&path)
                },
            );
            if let Err(error) = saved {
                self.chat_widget.add_warning_message(format!("Model selection applies to this session; project preferences could not be saved: {error}"));
            }
        }
        let effort = selection
            .effort
            .as_ref()
            .map(|effort| effort.as_str())
            .unwrap_or("service default");
        let timing = if was_running {
            " The running turn keeps its original settings; this selection applies from the next turn."
        } else {
            ""
        };
        self.chat_widget.add_info_message(
            format!(
                "{} mode: {} / {} · {effort}.{timing}",
                selection.mode.display_name(),
                selection.provider,
                selection.model
            ),
            None,
        );
        Ok(true)
    }

    async fn sync_confirmed_catalog_selection_to_sessions(
        &mut self,
        selection: &CatalogModelSelection,
    ) {
        let Some(thread_id) = self.active_thread_id else {
            return;
        };
        let collaboration_mode = self.chat_widget.effective_collaboration_mode();
        let update = |session: &mut crate::session_state::ThreadSessionState| {
            session.catalog_id = Some(selection.catalog_id.clone());
            session.model_provider_id = selection.provider.clone();
            if selection.mode == ModeKind::Default {
                session.model = selection.model.clone();
                session.reasoning_effort = selection.effort.clone();
            }
            session.collaboration_mode = Some(Box::new(collaboration_mode.clone()));
        };
        if self.primary_thread_id == Some(thread_id)
            && let Some(session) = self.primary_session_configured.as_mut()
        {
            update(session);
        }
        if let Some(blank) = self.agents_overview.blank_sessions.get_mut(&thread_id) {
            update(&mut blank.session);
        }
        if let Some(channel) = self.thread_event_channels.get(&thread_id) {
            let mut store = channel.store.lock().await;
            if let Some(session) = store.session.as_mut() {
                update(session);
            }
        }
    }

    pub(super) fn project_model_launch_overridden(&self) -> bool {
        self.harness_overrides.model.is_some()
            || self.harness_overrides.model_provider.is_some()
            || self.cli_kv_overrides.iter().any(|(key, _)| {
                matches!(
                    key.as_str(),
                    "model"
                        | "model_provider"
                        | "model_reasoning_effort"
                        | "plan_mode_reasoning_effort"
                )
            })
    }

    /// Fresh starts alone restore project preferences. CLI selections and explicit history win.
    pub(super) async fn restore_project_model(&mut self, app_server: &mut AppServerSession) {
        if self.project_model_launch_overridden() {
            return;
        }
        let path = crate::project_model_memory::memory_path(
            self.config.xdcode_home.as_path(),
            &self.chat_widget.catalog_config_path(),
            self.chat_widget.config_ref().cwd.as_path(),
        );
        let memory = match crate::project_model_memory::ProjectModelMemory::load(&path) {
            Ok(memory) => memory,
            Err(error) => {
                self.chat_widget.add_warning_message(format!(
                    "Could not restore project model preferences: {error}"
                ));
                return;
            }
        };
        self.chat_widget.restore_catalog_mode_memory(&memory);
        let mode = memory.last_mode.unwrap_or(ModeKind::Default);
        if let Some(mut selection) = memory.selection(mode).cloned() {
            selection.mode = mode;
            self.select_catalog_model(app_server, selection, false)
                .await;
        }
    }
}
