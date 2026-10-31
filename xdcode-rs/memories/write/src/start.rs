use crate::ensure_layout;
use crate::extensions::seed_extension_instructions;
use crate::guard;
use crate::metrics::MEMORY_STARTUP;
use crate::phase1;
use crate::phase2;
use crate::runtime::MemoryStartupContext;
use std::sync::Arc;
use tracing::warn;
use xdcode_core::ThreadManager;
use xdcode_core::XdcodeThread;
use xdcode_core::config::Config;
use xdcode_features::Feature;
use xdcode_login::AuthManager;
use xdcode_protocol::MemoryVersion;
use xdcode_protocol::ThreadId;
use xdcode_protocol::models::PermissionProfile;
use xdcode_protocol::protocol::SessionSource;

/// Starts the asynchronous startup memory pipeline for an eligible root session.
///
/// The pipeline is skipped for ephemeral sessions, disabled feature flags, and
/// subagent sessions.
pub fn start_memories_startup_task(
    thread_manager: Arc<ThreadManager>,
    auth_manager: Arc<AuthManager>,
    thread_id: ThreadId,
    thread: Arc<XdcodeThread>,
    config: Arc<Config>,
    parent_permission_profile: PermissionProfile,
    source: &SessionSource,
) {
    if config.ephemeral
        || !config.features.enabled(Feature::MemoryTool)
        || source.is_non_root_agent()
    {
        return;
    }

    // Dual-write warms v2 while v1 is selected; it is no longer needed after cutover.
    let versions = if config.memories.dual_write && config.memories.version == MemoryVersion::V1 {
        vec![MemoryVersion::V1, MemoryVersion::V2]
    } else {
        vec![config.memories.version]
    };
    for version in versions {
        let mut pipeline_config = config.as_ref().clone();
        pipeline_config.memories.version = version;
        let config = Arc::new(pipeline_config);
        let auth_manager = Arc::clone(&auth_manager);
        let parent_permission_profile = parent_permission_profile.clone();
        let context = Arc::new(MemoryStartupContext::new(
            Arc::clone(&thread_manager),
            Arc::clone(&auth_manager),
            thread_id,
            Arc::clone(&thread),
            config.as_ref(),
            source.clone(),
        ));
        tokio::spawn(async move {
            if context.memory_store().await.is_none() {
                warn!("state db unavailable for memories startup pipeline; skipping");
                return;
            }
            let root = config
                .xdcode_home
                .join(config.memories.version.directory_name());
            if let Err(err) = ensure_layout(&root).await {
                warn!("failed preparing memories root: {err}");
                return;
            }
            if let Err(err) = seed_extension_instructions(&root).await {
                warn!("failed seeding memory extension instructions: {err}");
            }

            // Clean memories to make preserve DB size. This does not consume tokens so can be
            // done before the quota check.
            phase1::prune(context.as_ref(), &config).await;

            if !guard::rate_limits_ok(&auth_manager, &config).await {
                context.counter(
                    MEMORY_STARTUP,
                    /*inc*/ 1,
                    &[("status", "skipped_rate_limit")],
                );
                return;
            }

            // Run phase 1.
            phase1::run(Arc::clone(&context), Arc::clone(&config)).await;
            // Run phase 2.
            phase2::run(context, config, parent_permission_profile).await;
        });
    }
}
