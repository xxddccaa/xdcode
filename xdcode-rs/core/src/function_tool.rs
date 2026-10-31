use std::future::Future;

use tokio_util::sync::CancellationToken;
use xdcode_async_utils::OrCancelExt;
pub use xdcode_tools::FunctionCallError;

/// Maps cancellation to the standard tool-call error while preserving the future's output.
pub(crate) trait OrCancelToolExt: OrCancelExt + Send {
    fn or_cancel_tool(
        self,
        token: &CancellationToken,
    ) -> impl Future<Output = Result<Self::Output, FunctionCallError>> + Send;
}

impl<F: OrCancelExt + Send> OrCancelToolExt for F {
    async fn or_cancel_tool(
        self,
        token: &CancellationToken,
    ) -> Result<Self::Output, FunctionCallError> {
        self.or_cancel(token)
            .await
            .map_err(|_| FunctionCallError::RespondToModel("tool call cancelled".to_string()))
    }
}
