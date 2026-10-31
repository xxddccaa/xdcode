//! Binds cached scores to the user authorization, review context, and model policy they evaluated.

use xdcode_core::GuardianAuthorizationVersion;
use xdcode_core::XdcodeThread;
use xdcode_core::context::GuardianReviewEvidence;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ScoreAuthorization {
    pub(super) permissions: xdcode_guardian_context::PermissionContext,
    pub(super) settings: xdcode_protocol::protocol::ThreadSettingsSnapshot,
    pub(super) environments: Vec<xdcode_protocol::protocol::TurnEnvironmentSelection>,
    pub(super) local: GuardianAuthorizationVersion,
    pub(super) review_context_revision: u64,
    pub(super) root: Option<GuardianAuthorizationVersion>,
    pub(super) root_review_context_revision: Option<u64>,
    pub(super) model: Option<std::sync::Arc<xdcode_protocol::model_types::ModelInfo>>,
}

impl ScoreAuthorization {
    pub(super) async fn is_current(&self, thread: &XdcodeThread) -> bool {
        self == &Self::current(thread, &self.permissions).await
    }

    pub(super) async fn current(
        thread: &XdcodeThread,
        permissions: &xdcode_guardian_context::PermissionContext,
    ) -> Self {
        let root = thread.guardian_root_snapshot().await;
        let history = thread.conversation_history_snapshot().await;
        let local = thread
            .thread_extension_data()
            .get_or_init(GuardianReviewEvidence::default)
            .authorization_version(history.as_ref());
        Self {
            permissions: permissions.clone(),
            settings: thread.thread_settings_snapshot().await,
            environments: thread
                .config_snapshot()
                .await
                .environment_selections()
                .to_vec(),
            local,
            review_context_revision: history.guardian_review_context_revision(),
            root: root.as_ref().map(|snapshot| snapshot.authorization_version),
            root_review_context_revision: root.map(|snapshot| snapshot.review_context_revision),
            model: thread
                .thread_extension_data()
                .get::<xdcode_protocol::model_types::ModelInfo>(),
        }
    }
}
