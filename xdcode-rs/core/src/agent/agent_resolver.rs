use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use std::sync::Arc;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::XdcodeErrorDetails;

/// Resolves a single tool-facing agent target to a thread id.
pub(crate) async fn resolve_agent_target(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    target: &str,
) -> Result<ThreadId, FunctionCallError> {
    session
        .services
        .agent_control
        .resolve(
            session.thread_id,
            turn.parent_thread_id,
            &turn.session_source,
            target,
        )
        .await
        .map_err(|err| match err.details() {
            XdcodeErrorDetails::UnsupportedOperation(message) => {
                FunctionCallError::RespondToModel(message.clone())
            }
            _ => FunctionCallError::RespondToModel(err.to_string()),
        })
}
