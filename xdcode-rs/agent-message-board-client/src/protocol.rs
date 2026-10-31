//! Versioned service requests reuse board operations; only host identity travels separately.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use xdcode_message_board::ChannelQuery;
use xdcode_message_board::CreateChannelRequest;
use xdcode_message_board::PostPreview;
use xdcode_message_board::PostQuery;
use xdcode_message_board::PostRequest;
use xdcode_message_board::ReadPostRequest;
use xdcode_message_board::ReadThreadRequest;
use xdcode_message_board::SubscriptionRequest;
use xdcode_message_board::ThreadQuery;
use xdcode_protocol::AgentPath;
use xdcode_protocol::ThreadId;

pub(crate) const MAX_BODY: usize = 512 * 1024;
pub(crate) const MAX_RESPONSE: usize = 1024 * 1024;

/// A host credential. Debug output must never reveal its contents.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct AccessToken(pub(crate) String);

impl AccessToken {
    pub fn new(value: String) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (32..=4096).contains(&value.len()),
            "credentials must contain 32–4096 bytes"
        );
        anyhow::ensure!(
            value.bytes().all(|b| b.is_ascii_graphic()),
            "credential must be ASCII without whitespace"
        );
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for AccessToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken([REDACTED])")
    }
}

#[derive(Serialize)]
pub(crate) struct Registration {
    pub(crate) members: HashMap<ThreadId, AgentPath>,
}

#[derive(Serialize)]
pub(crate) struct Call {
    pub(crate) caller: ThreadId,
    pub(crate) timestamp: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub(crate) operation: Operation,
}

#[derive(Serialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub(crate) enum Operation {
    CreateChannel(CreateChannelRequest),
    ListChannels(ChannelQuery),
    Post(PostRequest),
    ListThreads(ThreadQuery),
    SearchPosts(PostQuery),
    ReadThread(ReadThreadRequest),
    ReadPost(ReadPostRequest),
    SetSubscription(SubscriptionRequest),
}

#[derive(Serialize)]
pub(crate) struct Watch {
    pub(crate) caller: ThreadId,
    pub(crate) turn_id: String,
}

/// A best-effort preview bound to the receiving turn. Hosts must atomically check
/// `turn_id` when admitting it; a received preview must never start a new turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardNotification {
    pub recipient: ThreadId,
    pub turn_id: String,
    pub post: PostPreview,
}

#[derive(Deserialize)]
pub(crate) struct Failure {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Deserialize)]
pub(crate) struct PermissionDeniedDetails {
    pub(crate) action: Option<String>,
    pub(crate) resource: Option<String>,
}
