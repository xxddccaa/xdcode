//! Applies the rollout-gated CLI account check before resuming or attaching to a local thread.
//! Creator tags are immutable; untagged threads and other clients retain existing behavior.

use crate::error_code::invalid_request;
use xdcode_app_server_protocol::JSONRPCErrorError;
use xdcode_features::Feature;
use xdcode_features::Features;
use xdcode_login::AuthManager;
use xdcode_login::XdcodeAuth;
use xdcode_thread_store::StoredThread;

pub(super) fn check(
    features: &Features,
    client_name: Option<&str>,
    auth_manager: &AuthManager,
    thread: &StoredThread,
) -> Result<(), JSONRPCErrorError> {
    if !features.enabled(Feature::CliResumeAccountCheck)
        || !matches!(client_name, Some("xdcode-tui" | "xdcode_exec"))
        || thread.rollout_path.is_none()
        || (thread.creator_user_id.is_none() && thread.creator_account_id.is_none())
    {
        return Ok(());
    }
    let Some(auth) = auth_manager
        .auth_cached()
        .filter(XdcodeAuth::is_chatgpt_auth)
    else {
        // API keys and other auth modes do not identify a comparable ChatGPT account.
        return Ok(());
    };
    if thread
        .creator_user_id
        .as_ref()
        .is_some_and(|creator| auth.get_chatgpt_user_id().as_ref() != Some(creator))
        || thread
            .creator_account_id
            .as_ref()
            .is_some_and(|creator| auth.get_account_id().as_ref() != Some(creator))
    {
        return Err(invalid_request(
            "This thread was created with a different ChatGPT account or workspace. Sign in with the original account to resume it.",
        ));
    }
    Ok(())
}
