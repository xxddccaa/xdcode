//! Reads memory context from the loaded thread's resolved version, without fallback.

use super::*;
use xdcode_app_server_protocol::ThreadMemoryReadParams;
use xdcode_app_server_protocol::ThreadMemoryReadResponse;

impl ThreadRequestProcessor {
    pub(crate) async fn thread_memory_read(
        &self,
        params: ThreadMemoryReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_, thread) = self.load_thread(&params.thread_id).await?;
        let config = thread.config().await;
        let summary =
            if config.features.enabled(Feature::MemoryTool) && config.memories.use_memories {
                tokio::fs::read_to_string(
                    config
                        .xdcode_home
                        .join(config.memories.version.directory_name())
                        .join("memory_summary.md"),
                )
                .await
                .ok()
                .filter(|summary| !summary.trim().is_empty())
            } else {
                None
            };
        Ok(Some(ThreadMemoryReadResponse { summary }.into()))
    }
}
