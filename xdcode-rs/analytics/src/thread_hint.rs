//! Per-attempt thread hint status analytics without hint contents.

use crate::events::XdcodeAppServerClientMetadata;
use crate::events::XdcodeRuntimeMetadata;
use serde::Serialize;
use xdcode_protocol::protocol::ThreadSource;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHintStatus {
    Succeeded,
    Failed,
}

pub struct ThreadHintStatusEvent {
    pub thread_id: String,
    pub status: ThreadHintStatus,
    pub occurred_at_ms: u64,
}

#[derive(Serialize)]
pub(crate) struct ThreadHintStatusEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: ThreadHintStatusEventParams,
}

#[derive(Serialize)]
pub(crate) struct ThreadHintStatusEventParams {
    pub(crate) thread_id: String,
    pub(crate) session_id: String,
    pub(crate) app_server_client: XdcodeAppServerClientMetadata,
    pub(crate) runtime: XdcodeRuntimeMetadata,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) status: ThreadHintStatus,
    pub(crate) occurred_at_ms: u64,
}
