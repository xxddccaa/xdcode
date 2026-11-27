//! Host capabilities needed by a board without depending on xdcode-core.

use crate::PostPreview;
use chrono::DateTime;
use chrono::Utc;
use futures::future::BoxFuture;
use xdcode_protocol::AgentPath;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationDelivery {
    Accepted,
    SkippedInactive,
}

/// Board-scoped membership, clock and notification access supplied by the host.
///
/// The host remains authoritative for membership even when runtimes are unloaded.
/// Clock reads use the posting agent's configured source.
/// Notifications follow the host's ordinary queue-only agent-message scheduling
/// and wake policy, and may be buffered for an idle recipient.
/// Acceptance does not guarantee that the model has consumed the notification.
/// Backends may implement these capabilities with local handles or remote RPCs.
pub trait MessageBoardHost: Send + Sync {
    fn agent_path(&self, caller: ThreadId) -> BoxFuture<'_, Result<AgentPath>>;

    fn resolve_agent(&self, path: AgentPath) -> BoxFuture<'_, Result<ThreadId>>;

    fn current_time(&self, caller: ThreadId) -> BoxFuture<'_, Result<DateTime<Utc>>>;

    /// Push metadata and a bounded preview. Full content is retrieved through bounded reads.
    fn notify(
        &self,
        recipient: ThreadId,
        post: PostPreview,
    ) -> BoxFuture<'_, Result<NotificationDelivery>>;
}
