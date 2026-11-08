//! Publishes the existing in-memory launcher through ordinary MCP discovery.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::Mutex;
use xdcode_config::AppToolApproval;
use xdcode_config::DEFAULT_MCP_SERVER_ENVIRONMENT_ID;
use xdcode_config::McpServerConfig;
use xdcode_config::McpServerTransportConfig;
use xdcode_extension_api::CatalogRefreshInput;
use xdcode_extension_api::ExtensionData;
use xdcode_extension_api::ExtensionFuture;
use xdcode_extension_api::ExtensionRegistryBuilder;
use xdcode_extension_api::McpServerContribution;
use xdcode_extension_api::McpServerContributionContext;
use xdcode_extension_api::McpServerContributor;
use xdcode_extension_api::ThreadLifecycleContributor;
use xdcode_extension_api::ThreadStopInput;
use xdcode_extension_api::TurnLifecycleContributor;
use xdcode_message_board::MessageBoardHost;
use xdcode_message_board_mcp::LocalServer;
use xdcode_message_board_mcp::launch_in_memory;
use xdcode_protocol::SessionId;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result;
use xdcode_protocol::error::XdcodeErr;

/// Shared launchers opened by the existing board factory when the MCP flag is enabled.
#[derive(Default)]
pub struct McpBoards {
    servers: Mutex<HashMap<SessionId, Weak<LocalServer>>>,
}
struct Binding(Arc<LocalServer>);
struct PendingMcpRefresh;

impl McpBoards {
    pub async fn open(
        &self,
        store: &ExtensionData,
        tree: SessionId,
        caller: ThreadId,
        host: Arc<dyn MessageBoardHost>,
    ) -> Result<()> {
        let mut servers = self.servers.lock().await;
        servers.retain(|_, server| server.strong_count() > 0);
        let server = match servers.get(&tree).and_then(Weak::upgrade) {
            Some(server) => server,
            None => {
                let server = Arc::new(
                    launch_in_memory()
                        .await
                        .map_err(|error| XdcodeErr::Io(std::io::Error::other(error)))?,
                );
                servers.insert(tree, Arc::downgrade(&server));
                server
            }
        };
        server
            .bind_agent(tree, caller, host)
            .await
            .map_err(|error| XdcodeErr::Io(std::io::Error::other(error)))?;
        store.insert(Binding(server));
        store.insert(PendingMcpRefresh);
        Ok(())
    }
}

impl<C: Sync> ThreadLifecycleContributor<C> for McpBoards {
    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input.thread_store.remove::<Binding>();
        })
    }
}

impl TurnLifecycleContributor for McpBoards {
    fn refresh_catalog_for_step<'a>(
        &'a self,
        input: CatalogRefreshInput<'a>,
    ) -> ExtensionFuture<'a, bool> {
        Box::pin(async move { input.thread_store.remove::<PendingMcpRefresh>().is_some() })
    }
}

impl<C: Sync> McpServerContributor<C> for McpBoards {
    fn id(&self) -> &'static str {
        "agent_message_board_mcp"
    }

    fn contribute<'a>(
        &'a self,
        context: McpServerContributionContext<'a, C>,
    ) -> ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            let Some(binding) = context
                .thread_store()
                .and_then(|store| store.get::<Binding>())
            else {
                return Vec::new();
            };
            let Ok(authorization) = binding.0.authorization().to_str() else {
                return Vec::new();
            };
            vec![McpServerContribution::Set {
                name: "xdcode_message_board".into(),
                config: Box::new(McpServerConfig {
                    transport: McpServerTransportConfig::StreamableHttp {
                        url: binding.0.url().into(),
                        bearer_token_env_var: None,
                        http_headers: Some([("Authorization".into(), authorization.into())].into()),
                        env_http_headers: None,
                        http_headers_helper: None,
                    },
                    auth: Default::default(),
                    environment_id: DEFAULT_MCP_SERVER_ENVIRONMENT_ID.into(),
                    enabled: true,
                    required: true,
                    startup_readiness: Default::default(),
                    supports_parallel_tool_calls: false,
                    tool_input_schema_max_bytes: None,
                    omit_tools_from: None,
                    disabled_reason: None,
                    startup_timeout_sec: None,
                    tool_timeout_sec: None,
                    default_tools_approval_mode: Some(AppToolApproval::Approve),
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                    oauth: None,
                    oauth_resource: None,
                    tools: HashMap::new(),
                }),
            }]
        })
    }
}

/// Registers discovery for launchers created by the board factory.
pub fn install_mcp<C: Sync + 'static>(
    registry: &mut ExtensionRegistryBuilder<C>,
) -> Arc<McpBoards> {
    let boards = Arc::new(McpBoards::default());
    registry.thread_lifecycle_contributor(boards.clone());
    registry.turn_lifecycle_contributor(boards.clone());
    registry.mcp_server_contributor(boards.clone());
    boards
}
