use crate::session::turn_context::TurnContext;
use tracing::warn;
use xdcode_analytics::CompactionImplementation;
use xdcode_analytics::CompactionReason;
use xdcode_otel::SessionTelemetry;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::error::XdcodeErrorDetails;

/// Returns whether a failed compaction attempt should use the current model.
pub(crate) fn should_retry_with_current_model(
    error: &XdcodeErr,
    previous: &TurnContext,
    current: &TurnContext,
) -> bool {
    if matches!(
        error.details(),
        XdcodeErrorDetails::TurnAborted
            | XdcodeErrorDetails::Interrupted
            | XdcodeErrorDetails::SessionBudgetExceeded
            | XdcodeErrorDetails::ResponseIncomplete(_)
    ) || (previous.model_info().slug == current.model_info().slug
        && previous.cyber_access_program == current.cyber_access_program)
    {
        return false;
    }

    current.provider.info().is_hosted_provider()
        && current
            .auth_manager
            .as_deref()
            .and_then(xdcode_login::AuthManager::auth_cached)
            .is_some_and(|auth| auth.uses_xdcode_backend() || auth.is_api_key_auth())
}

pub(crate) fn record_model_fallback(
    session_telemetry: &SessionTelemetry,
    previous_model: &str,
    current_model: &str,
    reason: CompactionReason,
    implementation: CompactionImplementation,
    fallback_error: Option<&XdcodeErr>,
) {
    let reason_tag = match reason {
        CompactionReason::UserRequested => "user_requested",
        CompactionReason::ContextLimit => "context_limit",
        CompactionReason::ModelDownshift => "model_downshift",
        CompactionReason::CompHashChanged => "comp_hash_changed",
    };
    let implementation_tag = match implementation {
        CompactionImplementation::Responses => "responses",
        CompactionImplementation::ResponsesCompactionV2 => "responses_compaction_v2",
    };
    let outcome = if fallback_error.is_none() {
        "succeeded"
    } else {
        "failed"
    };
    session_telemetry.counter(
        "xdcode.compaction.model_fallback",
        /*inc*/ 1,
        &[
            ("reason", reason_tag),
            ("implementation", implementation_tag),
            ("outcome", outcome),
        ],
    );
    warn!(
        previous_model,
        current_model,
        ?reason,
        ?implementation,
        outcome,
        ?fallback_error,
        "previous-model compaction failed; retried with current model"
    );
}
