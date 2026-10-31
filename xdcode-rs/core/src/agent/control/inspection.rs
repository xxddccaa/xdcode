//! Reads registered agent identity, status and settings without restoring a runtime.
//! Known unloaded agents stay distinct from missing identities and backend failures.

use super::LocalAgentControl;
use crate::agent::api::AgentInfo;
use crate::agent::types::LiveAgent;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErrorDetails;

impl LocalAgentControl {
    pub(super) async fn inspect_agent(&self, thread_id: ThreadId) -> XdcodeResult<AgentInfo> {
        let manager = self.runtime.upgrade()?;
        let thread = match manager.get_thread(thread_id).await {
            Ok(thread) => thread,
            Err(err) if matches!(err.details(), XdcodeErrorDetails::ThreadNotFound(_)) => {
                return Ok(AgentInfo::Unloaded(
                    self.runtime.ensure_agent_known(thread_id)?,
                ));
            }
            Err(err) => return Err(err),
        };
        Ok(AgentInfo::Loaded {
            agent: LiveAgent {
                thread_id,
                metadata: self.get_agent_metadata(thread_id).unwrap_or_default(),
                status: thread.agent_status().await,
            },
            config: Box::new(thread.config_snapshot().await),
        })
    }
}
