//! Reopens an agent only when its runtime is absent, then returns its effective state.
//! Restores recorded ancestor ownership before loading a suspended descendant.

use super::LocalAgentControl;
use crate::XdcodeThread;
use crate::agent::types::LiveAgent;
use crate::config::Config;
use crate::thread_manager::LoadedSuspendedAgent;
use crate::xdcode_thread::ThreadConfigSnapshot;
use std::collections::HashSet;
use std::sync::Arc;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::error::XdcodeErrorDetails;
use xdcode_protocol::protocol::MultiAgentVersion;
use xdcode_protocol::protocol::SessionSource;
use xdcode_protocol::protocol::SubAgentSource;
use xdcode_thread_store::ReadThreadParams;

impl LocalAgentControl {
    /// Load ancestors first so each child inherits its actual parent's runtime and permissions.
    pub(crate) async fn load_suspended_child(
        &self,
        root: &Arc<XdcodeThread>,
        child_id: ThreadId,
    ) -> XdcodeResult<LoadedSuspendedAgent> {
        let root_id = root.startup_metadata().thread_id;
        if child_id == root_id || root.multi_agent_version() != Some(MultiAgentVersion::V2) {
            return Err(XdcodeErr::InvalidRequest(
                "expected a named V2 descendant".to_owned(),
            ));
        }
        let state = self.runtime.upgrade()?;
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut id = child_id;
        while id != root_id {
            if !visited.insert(id) {
                return Err(XdcodeErr::InvalidRequest(
                    "cycle in stored agent ancestry".into(),
                ));
            }
            let stored = state
                .read_stored_thread(ReadThreadParams {
                    thread_id: id,
                    include_archived: false,
                    include_history: false,
                })
                .await?;
            let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id,
                agent_path: Some(path),
                ..
            }) = &stored.source
            else {
                return Err(XdcodeErr::InvalidRequest(format!(
                    "thread {id} is not a named child of {root_id}"
                )));
            };
            if stored
                .parent_thread_id
                .is_some_and(|parent| parent != *parent_thread_id)
                || stored
                    .agent_path
                    .as_deref()
                    .is_some_and(|name| name != path.as_str())
            {
                return Err(XdcodeErr::InvalidRequest(format!(
                    "inconsistent stored ancestry for {id}"
                )));
            }
            chain.push(id);
            id = *parent_thread_id;
        }
        chain.reverse();
        let mut thread = Arc::clone(root);
        let mut residency = None;
        for id in chain {
            let config = thread.session.get_config().await;
            // Usually root resume registered the name from the graph. This also handles
            // stores without a graph, using the same identity restoration path.
            self.restore_agent_identities(&config, &state, vec![id])
                .await;
            let config = config.as_ref().clone();
            let pin = self
                .ensure_v2_agent_loaded(config, id, Some(thread))
                .await?;
            thread = state.get_thread(id).await?;
            // Allow an idle ancestor to give up its slot while its child reloads at capacity.
            if id == child_id {
                residency = pin;
            }
        }
        Ok(LoadedSuspendedAgent {
            thread,
            _residency: residency,
        })
    }

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
