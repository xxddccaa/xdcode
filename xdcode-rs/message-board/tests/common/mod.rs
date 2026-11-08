//! Shared host fixture for board and HTTP integration tests.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use chrono::DateTime;
use chrono::Utc;
use xdcode_message_board::*;
use xdcode_protocol::AgentPath;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::error::Result;
use futures::future::BoxFuture;

pub struct Host {
    pub clock: AtomicI64,
    pub fail_clock: AtomicBool,
    pub agent_path_calls: AtomicUsize,
    pub members: HashMap<ThreadId, AgentPath>,
    pub active: AtomicBool,
    pub fail_notifications: AtomicBool,
    pub notifications: Mutex<Vec<(ThreadId, PostMetadata)>>,
}

impl MessageBoardHost for Host {
    fn agent_path(&self, caller: ThreadId) -> BoxFuture<'_, Result<AgentPath>> {
        Box::pin(async move {
            self.agent_path_calls.fetch_add(1, Ordering::SeqCst);
            self.members
                .get(&caller)
                .cloned()
                .ok_or_else(|| XdcodeErr::ThreadNotFound(caller))
        })
    }

    fn resolve_agent(&self, path: AgentPath) -> BoxFuture<'_, Result<ThreadId>> {
        Box::pin(async move {
            self.members
                .iter()
                .find_map(|(id, member)| (*member == path).then_some(*id))
                .ok_or_else(|| XdcodeErr::InvalidRequest("unknown agent".into()))
        })
    }

    fn current_time(&self, _caller: ThreadId) -> BoxFuture<'_, Result<DateTime<Utc>>> {
        Box::pin(async move {
            if self.fail_clock.load(Ordering::SeqCst) {
                return Err(XdcodeErr::Io(std::io::Error::other("clock unavailable")));
            }
            Ok(DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
                .map_err(|error| XdcodeErr::Io(std::io::Error::other(error)))?
                .with_timezone(&Utc)
                + chrono::Duration::seconds(self.clock.fetch_add(1, Ordering::SeqCst)))
        })
    }

    fn notify(
        &self,
        recipient: ThreadId,
        post: PostPreview,
    ) -> BoxFuture<'_, Result<NotificationDelivery>> {
        Box::pin(async move {
            if self.fail_notifications.load(Ordering::SeqCst) {
                return Err(XdcodeErr::Io(std::io::Error::other(
                    "notification transport failed",
                )));
            }
            if !self.active.load(Ordering::SeqCst) {
                return Ok(NotificationDelivery::SkippedInactive);
            }
            self.notifications
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((recipient, post.metadata));
            Ok(NotificationDelivery::Accepted)
        })
    }
}
