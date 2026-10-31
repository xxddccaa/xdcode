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
/// Clock reads use the posting agent's configured source, including simulated
/// time. Notifications are accepted only while a running turn still accepts
/// mailbox delivery; otherwise they are skipped without being queued.
/// Accepted notifications must not start a new turn or reopen a finalized answer,
/// but may remain in conversation history for a later turn.
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
