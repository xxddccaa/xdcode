//! Captures admitted executor hooks from this step's discovery and MCP policy.

use xdcode_connectors::AppToolPolicyEvaluator;
use xdcode_connectors::AppToolPolicyInput;
use xdcode_core_plugins::executor_plugin_hook_sources;
use xdcode_core_plugins::executor_plugin_hook_sources_v2;
use xdcode_exec_server::ExecutorCapabilityDiscoverySnapshot;
use xdcode_exec_server::ExecutorCapabilityDiscoveryV2Snapshot;
use xdcode_extension_api::SelectedPluginSnapshot;
use xdcode_mcp::McpBinding;
use xdcode_mcp::XDCODE_APPS_MCP_SERVER_NAME;
use xdcode_plugin::ExecutorPluginHookSource;

pub(crate) fn prepare_executor_hook_sources(
    mcp: &McpBinding,
    selected_plugins: &SelectedPluginSnapshot,
    discovery: Option<&ExecutorCapabilityDiscoverySnapshot>,
    discovery_v2: Option<&[ExecutorCapabilityDiscoveryV2Snapshot]>,
) -> Vec<ExecutorPluginHookSource> {
    let config = mcp.config();
    let app_tool_policy = AppToolPolicyEvaluator::new(&config.config_layer_stack);
    let lookup_enabled_tool = |server: &str, tool: &str| {
        mcp.tool_info(server, tool).filter(|tool_info| {
            if server != XDCODE_APPS_MCP_SERVER_NAME {
                return true;
            }
            let annotations = tool_info.tool.annotations.as_ref();
            app_tool_policy
                .policy(AppToolPolicyInput {
                    connector_id: tool_info.connector_id.as_deref(),
                    link_id: None,
                    tool_name: &tool_info.tool.name,
                    tool_title: tool_info.tool.title.as_deref(),
                    destructive_hint: annotations
                        .and_then(|annotations| annotations.destructive_hint),
                    open_world_hint: annotations
                        .and_then(|annotations| annotations.open_world_hint),
                })
                .enabled
        })
    };
    let sources = if let Some(snapshots) = discovery_v2 {
        executor_plugin_hook_sources_v2(snapshots, &config.plugins, lookup_enabled_tool)
            .into_iter()
            .filter(|source| {
                let root_id = format!("{}:{}", source.environment_id, source.plugin_id.as_key());
                selected_plugins
                    .plugins
                    .iter()
                    .any(|plugin| plugin.selected_root_id.as_deref() == Some(root_id.as_str()))
            })
            .collect()
    } else if let Some(snapshot) = discovery {
        executor_plugin_hook_sources(snapshot, &config.plugins, lookup_enabled_tool)
    } else {
        Vec::new()
    };
    sources
        .into_iter()
        .filter(|source| {
            !selected_plugins
                .disabled_plugin_ids
                .contains(&source.plugin_id.as_key())
        })
        .collect()
}
