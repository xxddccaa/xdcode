use std::collections::BTreeMap;
use xdcode_protocol::model_types::ToolMessage;
use xdcode_tools::JsonSchema;
use xdcode_tools::ResponsesApiTool;
use xdcode_tools::ToolSpec;
use xdcode_tools::with_catalog_tool_message;

pub(crate) const NEW_CONTEXT_WINDOW_TOOL_NAME: &str = "new_context";

pub fn create_new_context_window_tool(messages: Option<&ToolMessage>) -> ToolSpec {
    let tool = ResponsesApiTool {
        name: NEW_CONTEXT_WINDOW_TOOL_NAME.to_string(),
        description: "Start a new context window. Does not clear, reset, or otherwise affect environment state.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), /*required*/ None, Some(false.into())),
        output_schema: None,
    };
    ToolSpec::Function(with_catalog_tool_message(tool, messages))
}
