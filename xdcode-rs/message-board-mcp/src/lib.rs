//! MCP adapters and HTTP transport for existing board tools.
//! Bound adapters fix caller identity; HTTP validates it against the configured host.
//! Storage, permissions and notification delivery remain owned by the board.

mod http;
mod local;

pub use http::serve_http;
pub use local::LocalServer;
pub use local::launch_in_memory;

use rmcp::ErrorData;
use rmcp::ServerHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResponse;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::Implementation;
use rmcp::model::ListToolsResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::model::Tool;
use rmcp::service::RequestContext;
use rmcp::service::RoleServer;
use std::sync::Arc;
use uuid::Uuid;
use xdcode_attachment_store::InlineAttachmentStore;
use xdcode_message_board::AgentMessageBoard;
use xdcode_message_board::MessageBoardTool;
use xdcode_message_board::message_board_tools;
use xdcode_protocol::AgentPath;
use xdcode_protocol::ThreadId;
use xdcode_tools::ConversationHistory;
use xdcode_tools::FunctionCallError;
use xdcode_tools::NoopTurnItemEmitter;
use xdcode_tools::ToolCall;
use xdcode_tools::ToolCallSource;
use xdcode_tools::ToolExecutor;
use xdcode_tools::ToolPayload;
use xdcode_tools::ToolSpec;
use xdcode_utils_output_truncation::TruncationPolicy;

/// A caller-bound MCP service, usable with any `rmcp` server transport.
///
/// Create one instance per MCP session. The host must authenticate the caller
/// and supply its authoritative path and board connection; request arguments or
/// metadata never select an identity. Selecting tools does not grant permissions.
/// The board's host continues to handle notifications; this service adds no
/// Core delivery, lifecycle registration, or MCP push notifications.
pub struct MessageBoardMcpServer {
    tools: Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>>,
    invocation_scope: String,
}

impl MessageBoardMcpServer {
    pub fn new(
        tools: &[MessageBoardTool],
        board: Arc<dyn AgentMessageBoard>,
        caller: ThreadId,
        caller_path: AgentPath,
    ) -> Self {
        Self {
            tools: message_board_tools(tools, board, caller, caller_path, None, "", None),
            invocation_scope: Uuid::now_v7().to_string(),
        }
    }
}

impl ServerHandler for MessageBoardMcpServer {
    fn get_info(&self) -> ServerConfig {
        server_info()
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        list_tools_result(request, self.tools.iter().map(|executor| executor.spec()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let executor = self
            .tools
            .iter()
            .find(|executor| executor.tool_name().name == request.name)
            .ok_or_else(|| ErrorData::invalid_params("Unknown message-board tool", None))?;
        let call_id = serde_json::to_string(&context.id)
            .map_err(|_| ErrorData::internal_error("Invalid MCP request ID", None))?;
        let payload = ToolPayload::Function {
            arguments: serde_json::Value::Object(request.arguments.unwrap_or_default()).to_string(),
        };
        let call = ToolCall {
            turn_id: self.invocation_scope.clone(),
            call_id,
            tool_name: executor.tool_name(),
            model: String::new(),
            xdcode_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(usize::MAX),
            source: ToolCallSource::Direct,
            conversation_history: ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            attachment_store: Arc::new(InlineAttachmentStore),
            ephemeral: false,
            environments: Vec::new(),
            payload: payload.clone(),
        };
        let result = match executor.handle(call).await {
            Ok(output) => {
                let value = output.code_mode_result(&payload);
                let mut result = if value.is_object() {
                    CallToolResult::structured(value)
                } else {
                    CallToolResult::success(vec![ContentBlock::text(value.to_string())])
                };
                result.is_error = Some(!output.success_for_logging());
                result
            }
            Err(FunctionCallError::RespondToModel(message)) => {
                CallToolResult::error(vec![ContentBlock::text(message)])
            }
            Err(FunctionCallError::Fatal(_)) => {
                return Err(ErrorData::internal_error("Message-board tool failed", None));
            }
        };
        Ok(result.into())
    }
}

fn server_info() -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
        Implementation::new("xdcode-message-board", env!("CARGO_PKG_VERSION")),
    )
}

fn list_tools_result(
    request: Option<PaginatedRequestParams>,
    specs: impl Iterator<Item = ToolSpec>,
) -> Result<ListToolsResult, ErrorData> {
    if request.is_some_and(|request| request.cursor.is_some()) {
        return Err(ErrorData::invalid_params(
            "Message-board tools do not use pagination cursors",
            None,
        ));
    }
    let tools = specs
        .map(|spec| {
            let ToolSpec::Function(spec) = spec else {
                return Err(ErrorData::internal_error(
                    "Message-board tool is not a function",
                    None,
                ));
            };
            let schema = serde_json::to_value(spec.parameters)
                .and_then(serde_json::from_value)
                .map_err(|_| ErrorData::internal_error("Invalid message-board schema", None))?;
            Ok(Tool::new(spec.name, spec.description, Arc::new(schema)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ListToolsResult::with_all_items(tools))
}
