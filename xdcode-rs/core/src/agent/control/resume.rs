//! Reopens an agent only when its runtime is absent, then returns its effective state.
//! Rollout and descendant restoration remain in the existing local startup path.

use super::LocalAgentControl;
use crate::agent::types::LiveAgent;
use crate::config::Config;
use crate::xdcode_thread::ThreadConfigSnapshot;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErrorDetails;
use xdcode_protocol::protocol::SessionSource;

impl LocalAgentControl {
    pub(crate) async fn resume_agent(
        &self,
        config: Config,
        thread_id: ThreadId,
        source: SessionSource,
    ) -> XdcodeResult<(LiveAgent, ThreadConfigSnapshot)> {
        let manager = self.runtime.upgrade()?;
        let thread = match manager.get_thread(thread_id).await {
            Ok(thread) => thread,
            Err(err) if matches!(err.details(), XdcodeErrorDetails::ThreadNotFound(_)) => {
                Box::pin(self.resume_agent_from_rollout(config, thread_id, source)).await?;
                manager.get_thread(thread_id).await?
            }
            Err(err) => return Err(err),
        };
        let agent = LiveAgent {
            thread_id,
            metadata: self.get_agent_metadata(thread_id).unwrap_or_default(),
            status: thread.agent_status().await,
        };
        let config = thread.config_snapshot().await;
        Ok((agent, config))
    }
}
