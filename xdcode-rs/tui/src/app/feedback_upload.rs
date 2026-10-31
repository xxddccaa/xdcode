//! Retained response type for clients; independent builds never stage or upload feedback.

use color_eyre::eyre::Result;
use xdcode_app_server_client::{AppServerPath, AppServerRequestHandle};
use xdcode_app_server_protocol::{FeedbackUploadParams, FeedbackUploadResponse};
use xdcode_feedback::XdcodeFeedback;

pub(super) async fn fetch_feedback_upload(
    _request_handle: AppServerRequestHandle,
    _xdcode_home: Option<AppServerPath>,
    _params: FeedbackUploadParams,
    _feedback: XdcodeFeedback,
) -> Result<FeedbackUploadResponse> {
    color_eyre::eyre::bail!("Feedback uploads are unavailable in xdcode")
}
