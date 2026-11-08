//! Canonical TUI session state shared across app-server routing, chat display, and status UI.
//!
//! The app-server API is the boundary for session lifecycle events. Once those responses enter
//! TUI, this module holds the small internal state shape used by app orchestration and widgets.

use std::path::PathBuf;

use xdcode_app_server_protocol::AskForApproval;
use xdcode_protocol::ThreadId;
use xdcode_protocol::config_types::CollaborationMode;
use xdcode_protocol::config_types::ModeKind;
use xdcode_protocol::config_types::Settings;
use xdcode_protocol::models::ActivePermissionProfile;
use xdcode_protocol::models::PermissionProfile;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_path_uri::PathUri;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionNetworkProxyRuntime {
    pub(crate) http_addr: String,
    pub(crate) socks_addr: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MessageHistoryMetadata {
    pub(crate) log_id: u64,
    pub(crate) entry_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NewSessionModelSelection {
    pub(crate) provider: String,
    pub(crate) catalog_id: Option<String>,
    pub(crate) collaboration_mode: CollaborationMode,
}

impl NewSessionModelSelection {
    pub(crate) fn from_session(session: &ThreadSessionState) -> Self {
        let mut collaboration_mode = session
            .collaboration_mode
            .as_deref()
            .cloned()
            .unwrap_or_else(|| CollaborationMode {
                mode: ModeKind::Default,
                settings: Settings {
                    model: session.model.clone(),
                    reasoning_effort: session.reasoning_effort.clone(),
                    developer_instructions: None,
                },
            });
        collaboration_mode.settings.model = session.model.clone();
        collaboration_mode.settings.reasoning_effort = session.reasoning_effort.clone();
        collaboration_mode.settings.developer_instructions = None;
        Self {
            provider: session.model_provider_id.clone(),
            catalog_id: session.catalog_id.clone(),
            collaboration_mode,
        }
    }

    pub(crate) fn update_params(
        &self,
        thread_id: ThreadId,
    ) -> xdcode_app_server_protocol::ThreadSettingsUpdateParams {
        let mut params = xdcode_app_server_protocol::ThreadSettingsUpdateParams {
            thread_id: thread_id.to_string(),
            ..Default::default()
        };
        if let Some(catalog_id) = &self.catalog_id {
            params.catalog_selection = Some(xdcode_protocol::protocol::CatalogModelSelection {
                provider: self.provider.clone(),
                catalog_id: catalog_id.clone(),
                model: self.collaboration_mode.settings.model.clone(),
                effort: self.collaboration_mode.settings.reasoning_effort.clone(),
                mode: self.collaboration_mode.mode,
            });
        } else {
            params.model = Some(self.collaboration_mode.settings.model.clone());
            params.effort = Some(self.collaboration_mode.settings.reasoning_effort.clone());
            params.collaboration_mode = Some(self.collaboration_mode.clone());
        }
        params
    }

    pub(crate) fn apply_to_config(&self, config: &mut crate::legacy_core::config::Config) {
        config.model = Some(self.collaboration_mode.settings.model.clone());
        config.model_provider_id = self.provider.clone();
        if let Some(provider) = config.model_providers.get(&self.provider) {
            config.model_provider = provider.clone();
        }
        config.model_reasoning_effort = self.collaboration_mode.settings.reasoning_effort.clone();
    }

    pub(crate) fn apply_to_session(&self, session: &mut ThreadSessionState) {
        session.model_provider_id = self.provider.clone();
        session.catalog_id = self.catalog_id.clone();
        session.model = self.collaboration_mode.settings.model.clone();
        session.reasoning_effort = self.collaboration_mode.settings.reasoning_effort.clone();
        session.collaboration_mode = Some(Box::new(self.collaboration_mode.clone()));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadSessionState {
    pub(crate) catalog_id: Option<String>,
    pub(crate) daybreak_enabled: bool,
    pub(crate) windows_sandbox_host: crate::app::WindowsSandboxHost,
    pub(crate) thread_id: ThreadId,
    pub(crate) forked_from_id: Option<ThreadId>,
    pub(crate) fork_parent_title: Option<String>,
    pub(crate) thread_name: Option<String>,
    pub(crate) model: String,
    pub(crate) model_provider_id: String,
    pub(crate) service_tier: Option<String>,
    pub(crate) approval_policy: AskForApproval,
    pub(crate) approvals_reviewer: xdcode_protocol::config_types::ApprovalsReviewer,
    /// Permission snapshot used by TUI display surfaces. Legacy app-server
    /// responses are converted to a profile at ingestion time using the
    /// response cwd so cached sessions do not reinterpret cwd-bound grants.
    /// Turn requests must not treat this snapshot as a local permission
    /// override unless the user explicitly changed permissions in the TUI.
    pub(crate) permission_profile: PermissionProfile,
    /// Named or implicit built-in profile that produced `permission_profile`,
    /// when the server knows it.
    pub(crate) active_permission_profile: Option<ActivePermissionProfile>,
    pub(crate) cwd: AbsolutePathBuf,
    pub(crate) runtime_workspace_roots: Vec<AbsolutePathBuf>,
    pub(crate) instruction_source_paths: Vec<PathUri>,
    pub(crate) reasoning_effort: Option<xdcode_protocol::model_types::ReasoningEffort>,
    pub(crate) collaboration_mode: Option<Box<CollaborationMode>>,
    pub(crate) message_history: Option<MessageHistoryMetadata>,
    pub(crate) network_proxy: Option<SessionNetworkProxyRuntime>,
    pub(crate) rollout_path: Option<PathBuf>,
}

impl ThreadSessionState {
    pub(crate) fn set_cwd_retargeting_implicit_runtime_workspace_root(
        &mut self,
        cwd: AbsolutePathBuf,
    ) {
        let previous_cwd = std::mem::replace(&mut self.cwd, cwd.clone());
        if !self.runtime_workspace_roots.contains(&previous_cwd) {
            return;
        }

        let previous_roots = std::mem::take(&mut self.runtime_workspace_roots);
        self.runtime_workspace_roots.push(cwd);
        for root in previous_roots {
            if root != previous_cwd && !self.runtime_workspace_roots.contains(&root) {
                self.runtime_workspace_roots.push(root);
            }
        }
    }
}
