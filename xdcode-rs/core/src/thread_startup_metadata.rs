//! Retained startup metadata for new and resumed threads.

use std::path::PathBuf;
use xdcode_protocol::SessionId;
use xdcode_protocol::ThreadId;
use xdcode_protocol::config_types::ApprovalsReviewer;
use xdcode_protocol::model_types::ReasoningEffort;
use xdcode_protocol::models::ActivePermissionProfile;
use xdcode_protocol::models::PermissionProfile;
use xdcode_protocol::protocol::AskForApproval;
use xdcode_protocol::protocol::SessionConfiguredEvent;
use xdcode_protocol::protocol::SessionNetworkProxyRuntime;
use xdcode_protocol::protocol::ThreadSource;
use xdcode_utils_absolute_path::AbsolutePathBuf;

/// The configuration reported when a thread started.
/// Current settings are available through [`crate::XdcodeThread::config_snapshot`].
#[derive(Debug)]
pub struct ThreadStartupMetadata {
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    forked_from_id: Option<ThreadId>,
    parent_thread_id: Option<ThreadId>,
    thread_source: Option<ThreadSource>,
    thread_name: Option<String>,
    model: String,
    model_provider_id: String,
    service_tier: Option<String>,
    approval_policy: AskForApproval,
    approvals_reviewer: ApprovalsReviewer,
    permission_profile: PermissionProfile,
    active_permission_profile: Option<ActivePermissionProfile>,
    cwd: AbsolutePathBuf,
    reasoning_effort: Option<ReasoningEffort>,
    network_proxy: Option<SessionNetworkProxyRuntime>,
    rollout_path: Option<PathBuf>,
}

impl From<&SessionConfiguredEvent> for ThreadStartupMetadata {
    fn from(event: &SessionConfiguredEvent) -> Self {
        let SessionConfiguredEvent {
            session_id,
            thread_id,
            forked_from_id,
            parent_thread_id,
            thread_source,
            thread_name,
            model,
            model_provider_id,
            service_tier,
            approval_policy,
            approvals_reviewer,
            permission_profile,
            active_permission_profile,
            cwd,
            reasoning_effort,
            network_proxy,
            rollout_path,
        } = event;
        Self {
            session_id: *session_id,
            thread_id: *thread_id,
            forked_from_id: *forked_from_id,
            parent_thread_id: *parent_thread_id,
            thread_source: thread_source.clone(),
            thread_name: thread_name.clone(),
            model: model.clone(),
            model_provider_id: model_provider_id.clone(),
            service_tier: service_tier.clone(),
            approval_policy: *approval_policy,
            approvals_reviewer: *approvals_reviewer,
            permission_profile: permission_profile.clone(),
            active_permission_profile: active_permission_profile.clone(),
            cwd: cwd.clone(),
            reasoning_effort: reasoning_effort.clone(),
            network_proxy: network_proxy.clone(),
            rollout_path: rollout_path.clone(),
        }
    }
}

impl ThreadStartupMetadata {
    pub(crate) fn to_session_configured_event(&self) -> SessionConfiguredEvent {
        SessionConfiguredEvent {
            session_id: self.session_id,
            thread_id: self.thread_id,
            forked_from_id: self.forked_from_id,
            parent_thread_id: self.parent_thread_id,
            thread_source: self.thread_source.clone(),
            thread_name: self.thread_name.clone(),
            model: self.model.clone(),
            model_provider_id: self.model_provider_id.clone(),
            service_tier: self.service_tier.clone(),
            approval_policy: self.approval_policy,
            approvals_reviewer: self.approvals_reviewer,
            permission_profile: self.permission_profile.clone(),
            active_permission_profile: self.active_permission_profile.clone(),
            cwd: self.cwd.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            network_proxy: self.network_proxy.clone(),
            rollout_path: self.rollout_path.clone(),
        }
    }
}
