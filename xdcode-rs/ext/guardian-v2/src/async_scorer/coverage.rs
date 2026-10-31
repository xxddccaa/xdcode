//! Selects tool calls for classification using the resolved model policy.

use xdcode_extension_api::ToolPayload;
use xdcode_protocol::ToolName;
use xdcode_protocol::model_types::GuardianModelPolicy;
use xdcode_protocol::model_types::GuardianReviewMode::Adaptive;
use xdcode_protocol::model_types::GuardianScope;

pub(super) fn scores_tool(
    policy: &GuardianModelPolicy,
    tool: &ToolName,
    payload: &ToolPayload,
    scope: Option<GuardianScope>,
) -> bool {
    if scope.map_or(policy.other_tools, |scope| policy.review_mode(scope)) != Adaptive {
        return false;
    }
    if policy.sandboxed_exec_commands.unwrap_or(/*default*/ true)
        || !tool.is_default_namespace()
        || tool.name != "exec_command"
    {
        return true;
    }
    matches!(payload, ToolPayload::Function { arguments }
    if serde_json::from_str::<serde_json::Value>(arguments).ok().is_some_and(|arguments| {
        arguments.get("sandbox_permissions").and_then(serde_json::Value::as_str)
            == Some("require_escalated")
    }))
}
