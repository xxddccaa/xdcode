use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use xdcode_protocol::model_types::ToolMessage;
use xdcode_tools::JsonSchema;
use xdcode_tools::ResponsesApiTool;
use xdcode_tools::ToolSpec;
use xdcode_tools::with_catalog_tool_message;

pub(crate) const GET_CONTEXT_REMAINING_TOOL_NAME: &str = "get_context_remaining";

pub fn create_get_context_remaining_tool(messages: Option<&ToolMessage>) -> ToolSpec {
    let tool = ResponsesApiTool {
        name: GET_CONTEXT_REMAINING_TOOL_NAME.to_string(),
        description: "Get the remaining tokens in the current context window.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(BTreeMap::new(), /*required*/ None, Some(false.into())),
        output_schema: Some(get_context_remaining_output_schema().into()),
    };
    ToolSpec::Function(with_catalog_tool_message(tool, messages))
}

fn get_context_remaining_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tokens_left": {
                "anyOf": [
                    { "type": "integer" },
                    { "type": "null" }
                ],
                "description": "Remaining tokens in the current context window, or null when unavailable."
            }
        },
        "required": ["tokens_left"],
        "additionalProperties": false
    })
}
