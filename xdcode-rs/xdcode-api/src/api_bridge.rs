use crate::TransportError;
use crate::error::ApiError;
use crate::error::parse_terminal_provider_error;
use crate::rate_limits::parse_promo_message;
use crate::rate_limits::parse_rate_limit_for_limit;
use crate::rate_limits::parse_rate_limit_reached_type;
use base64::Engine;
use chrono::DateTime;
use chrono::Utc;
use http::HeaderMap;
use serde::Deserialize;
use serde_json::Value;
use xdcode_protocol::auth::PlanType;
use xdcode_protocol::error::ConnectionFailedError;
use xdcode_protocol::error::RetryLimitReachedError;
use xdcode_protocol::error::UnexpectedResponseError;
use xdcode_protocol::error::UsageLimitReachedError;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::error::XdcodeErrorDetails;
use xdcode_protocol::protocol::MisalignmentErrorDetails;

pub fn map_api_error(err: ApiError) -> XdcodeErr {
    let retry_after = match &err {
        ApiError::Retryable { retry_after, .. }
        | ApiError::RateLimitExceeded { retry_after, .. }
        | ApiError::ServerOverloaded { retry_after }
        | ApiError::Transport(TransportError::Http { retry_after, .. }) => *retry_after,
        ApiError::Transport(_)
        | ApiError::Api { .. }
        | ApiError::Stream(_)
        | ApiError::ContentFilter
        | ApiError::ResponseIncomplete(_)
        | ApiError::ContextWindowExceeded
        | ApiError::QuotaExceeded
        | ApiError::UsageNotIncluded
        | ApiError::SessionBudgetExceeded
        | ApiError::FlexUnavailable
        | ApiError::RateLimit(_)
        | ApiError::InvalidRequest { .. }
        | ApiError::ModelNotFound { .. }
        | ApiError::InvalidPrompt { .. }
        | ApiError::CyberPolicy { .. }
        | ApiError::BioPolicy { .. }
        | ApiError::MisalignmentPolicyViolation { .. } => None,
    };
    let error = map_api_error_details(err);
    match retry_after {
        Some(retry_after) => error.with_retry_after(retry_after),
        None => error,
    }
}

fn map_api_error_details(err: ApiError) -> XdcodeErr {
    match err {
        ApiError::ContextWindowExceeded => XdcodeErr::ContextWindowExceeded,
        ApiError::QuotaExceeded => XdcodeErr::QuotaExceeded,
        ApiError::UsageNotIncluded => XdcodeErr::UsageNotIncluded,
        ApiError::SessionBudgetExceeded => XdcodeErr::SessionBudgetExceeded,
        ApiError::Retryable { message, .. } => XdcodeErr::Stream(message),
        ApiError::RateLimitExceeded { message, .. } => {
            XdcodeErr::new(XdcodeErrorDetails::RateLimitExceeded(message))
        }
        ApiError::Stream(msg) => XdcodeErr::Stream(msg),
        ApiError::ContentFilter => XdcodeErr::new(XdcodeErrorDetails::ContentFilter),
        ApiError::ResponseIncomplete(incomplete) => {
            XdcodeErr::new(XdcodeErrorDetails::ResponseIncomplete(incomplete))
        }
        ApiError::ServerOverloaded { .. } => XdcodeErr::ServerOverloaded,
        ApiError::FlexUnavailable => XdcodeErr::new(XdcodeErrorDetails::FlexUnavailable),
        ApiError::Api { status, message } => {
            let user_message = api_error_user_message(status, &message);
            XdcodeErr::UnexpectedStatus(UnexpectedResponseError {
                status,
                body: message,
                user_message,
                url: None,
                cf_ray: None,
                request_id: None,
                identity_authorization_error: None,
                identity_error_code: None,
            })
        }
        ApiError::InvalidRequest { message } => XdcodeErr::InvalidRequest(message),
        ApiError::ModelNotFound { message, status } => {
            XdcodeErr::new(XdcodeErrorDetails::ModelNotFound { message, status })
        }
        ApiError::InvalidPrompt { message } => {
            XdcodeErr::new(XdcodeErrorDetails::InvalidPrompt { message })
        }
        ApiError::CyberPolicy { message } => {
            XdcodeErr::new(XdcodeErrorDetails::CyberPolicy { message })
        }
        ApiError::BioPolicy { message } => {
            XdcodeErr::new(XdcodeErrorDetails::BioPolicy { message })
        }
        ApiError::MisalignmentPolicyViolation {
            message,
            misalignment,
        } => XdcodeErr::new(XdcodeErrorDetails::MisalignmentPolicyViolation {
            message,
            misalignment,
        }),
        ApiError::Transport(transport) => match transport {
            TransportError::Http {
                status,
                url,
                headers,
                body,
                ..
            } => {
                let body_text = body.unwrap_or_default();
                let parsed_body = serde_json::from_str::<Value>(&body_text).ok();
                let provider_error = parsed_body.as_ref().and_then(|body| body.get("error"));
                if let Some(error) = parse_terminal_provider_error(provider_error, Some(status)) {
                    return map_api_error_details(error);
                }

                if status == http::StatusCode::SERVICE_UNAVAILABLE
                    && let Some(error) = provider_error
                {
                    match error.get("code").and_then(Value::as_str) {
                        Some("server_is_overloaded") => return XdcodeErr::ServerOverloaded,
                        Some("slow_down") => {
                            return XdcodeErr::new(XdcodeErrorDetails::RateLimitExceeded(
                                error
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                            ));
                        }
                        _ => {}
                    }
                }

                if (status == http::StatusCode::BAD_REQUEST
                    || status == http::StatusCode::FORBIDDEN)
                    && let Some(error) = provider_error
                    && error.get("code").and_then(Value::as_str)
                        == Some(MISALIGNMENT_POLICY_VIOLATION_ERROR_CODE)
                {
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .filter(|message| !message.trim().is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            MISALIGNMENT_POLICY_VIOLATION_FALLBACK_MESSAGE.to_string()
                        });
                    return XdcodeErr::new(XdcodeErrorDetails::MisalignmentPolicyViolation {
                        message,
                        misalignment: error.get("misalignment").cloned().and_then(|details| {
                            serde_json::from_value::<MisalignmentErrorDetails>(details).ok()
                        }),
                    });
                }

                if status == http::StatusCode::BAD_REQUEST {
                    if let Some(error) = provider_error
                        && let Some(
                            code @ (CYBER_POLICY_ERROR_CODE
                            | BIO_POLICY_ERROR_CODE
                            | INVALID_PROMPT_ERROR_CODE),
                        ) = error.get("code").and_then(Value::as_str)
                    {
                        let fallback_message = if code == BIO_POLICY_ERROR_CODE {
                            BIO_POLICY_FALLBACK_MESSAGE
                        } else if code == INVALID_PROMPT_ERROR_CODE {
                            INVALID_PROMPT_FALLBACK_MESSAGE
                        } else {
                            CYBER_POLICY_FALLBACK_MESSAGE
                        };
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .filter(|message| !message.trim().is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| fallback_message.to_string());
                        if code == BIO_POLICY_ERROR_CODE {
                            XdcodeErr::new(XdcodeErrorDetails::BioPolicy { message })
                        } else if code == INVALID_PROMPT_ERROR_CODE {
                            XdcodeErr::new(XdcodeErrorDetails::InvalidPrompt { message })
                        } else {
                            XdcodeErr::new(XdcodeErrorDetails::CyberPolicy { message })
                        }
                    } else if body_text
                        .contains("The image data you provided does not represent a valid image")
                    {
                        XdcodeErr::InvalidImageRequest()
                    } else {
                        XdcodeErr::InvalidRequest(body_text)
                    }
                } else if status == http::StatusCode::INTERNAL_SERVER_ERROR {
                    XdcodeErr::InternalServerError
                } else if status == http::StatusCode::TOO_MANY_REQUESTS {
                    if let Ok(err) = serde_json::from_str::<UsageErrorResponse>(&body_text) {
                        if err.error.error_type.as_deref() == Some("usage_limit_reached") {
                            let limit_id = extract_header(headers.as_ref(), ACTIVE_LIMIT_HEADER);
                            let promo_message = headers.as_ref().and_then(parse_promo_message);
                            let rate_limit_reached_type =
                                headers.as_ref().and_then(parse_rate_limit_reached_type);
                            let rate_limits = headers
                                .as_ref()
                                .and_then(|map| {
                                    parse_rate_limit_for_limit(map, limit_id.as_deref())
                                })
                                .map(|mut snapshot| {
                                    snapshot.rate_limit_reached_type = rate_limit_reached_type;
                                    snapshot
                                });
                            let resets_at = err
                                .error
                                .resets_at
                                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
                            return XdcodeErr::UsageLimitReached(UsageLimitReachedError {
                                plan_type: err.error.plan_type,
                                resets_at,
                                limit_window_minutes: err
                                    .error
                                    .limit_window_minutes
                                    .as_ref()
                                    .and_then(Value::as_u64)
                                    .and_then(|minutes| u16::try_from(minutes).ok()),
                                rate_limits: rate_limits.map(Box::new),
                                promo_message,
                                rate_limit_reached_type,
                            });
                        } else if err.error.error_type.as_deref() == Some("usage_not_included") {
                            return XdcodeErr::UsageNotIncluded;
                        } else if err.error.error_type.as_deref() == Some("insufficient_quota")
                            || matches!(
                                err.error.code.as_deref(),
                                Some(
                                    "insufficient_quota"
                                        | "credit_balance_exhausted"
                                        | "organization_spend_limit_exceeded"
                                        | "project_spend_limit_exceeded"
                                        | "organization_usage_limit_exceeded"
                                )
                            )
                        {
                            return XdcodeErr::QuotaExceeded;
                        }
                    }

                    let message = serde_json::from_str::<Value>(&body_text)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("error")?
                                .get("message")?
                                .as_str()
                                .map(str::to_owned)
                        })
                        .filter(|message| !message.trim().is_empty())
                        .unwrap_or(body_text);
                    XdcodeErr::new(XdcodeErrorDetails::RateLimitExceeded(message))
                } else {
                    XdcodeErr::UnexpectedStatus(UnexpectedResponseError {
                        status,
                        user_message: api_error_user_message(status, &body_text),
                        body: body_text,
                        url,
                        cf_ray: extract_header(headers.as_ref(), CF_RAY_HEADER),
                        request_id: extract_request_id(headers.as_ref()),
                        identity_authorization_error: extract_header(
                            headers.as_ref(),
                            X_PROVIDER_AUTHORIZATION_ERROR_HEADER,
                        ),
                        identity_error_code: extract_x_error_json_code(headers.as_ref()),
                    })
                }
            }
            TransportError::RetryLimit => XdcodeErr::RetryLimit(RetryLimitReachedError {
                status: http::StatusCode::INTERNAL_SERVER_ERROR,
                request_id: None,
            }),
            TransportError::Timeout => XdcodeErr::RequestTimeout,
            TransportError::Policy(denied) => XdcodeErr::Fatal(denied.to_string()),
            TransportError::Connection(source) => {
                XdcodeErr::ConnectionFailed(ConnectionFailedError { source })
            }
            TransportError::Network(msg) | TransportError::Build(msg) => XdcodeErr::Stream(msg),
            error @ TransportError::ResponseTooLarge { .. } => {
                XdcodeErr::InvalidRequest(error.to_string())
            }
        },
        ApiError::RateLimit(msg) => XdcodeErr::Stream(msg),
    }
}

const ACTIVE_LIMIT_HEADER: &str = "x-xdcode-active-limit";
const REQUEST_ID_HEADER: &str = "x-request-id";
const OAI_REQUEST_ID_HEADER: &str = "x-oai-request-id";
const CF_RAY_HEADER: &str = "cf-ray";
const X_PROVIDER_AUTHORIZATION_ERROR_HEADER: &str = "x-openai-authorization-error";
const X_ERROR_JSON_HEADER: &str = "x-error-json";
const INVALID_PROMPT_ERROR_CODE: &str = "invalid_prompt";
const INVALID_PROMPT_FALLBACK_MESSAGE: &str = "Invalid request.";
const CYBER_POLICY_ERROR_CODE: &str = "cyber_policy";
const CYBER_POLICY_FALLBACK_MESSAGE: &str =
    "This request has been flagged for possible cybersecurity risk.";
const BIO_POLICY_ERROR_CODE: &str = "bio_policy";
const BIO_POLICY_FALLBACK_MESSAGE: &str = "This content was flagged for possible biological risk.";
const MISALIGNMENT_POLICY_VIOLATION_ERROR_CODE: &str = "misalignment_policy_violation";
const MISALIGNMENT_POLICY_VIOLATION_FALLBACK_MESSAGE: &str =
    "This request was blocked due to a misalignment policy violation.";
const CLOUDFLARE_BLOCKED_MESSAGE: &str =
    "Access blocked by Cloudflare. This usually happens when connecting from a restricted region";

#[cfg(test)]
#[path = "api_bridge_tests.rs"]
mod tests;

fn api_error_user_message(status: http::StatusCode, body: &str) -> Option<String> {
    if status == http::StatusCode::FORBIDDEN
        && body.contains("Cloudflare")
        && body.contains("blocked")
    {
        Some(format!("{CLOUDFLARE_BLOCKED_MESSAGE} (status {status})"))
    } else {
        None
    }
}

fn extract_request_id(headers: Option<&HeaderMap>) -> Option<String> {
    extract_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_header(headers, OAI_REQUEST_ID_HEADER))
}

fn extract_header(headers: Option<&HeaderMap>, name: &str) -> Option<String> {
    headers.and_then(|map| {
        map.get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    })
}

fn extract_x_error_json_code(headers: Option<&HeaderMap>) -> Option<String> {
    let encoded = extract_header(headers, X_ERROR_JSON_HEADER)?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let parsed = serde_json::from_slice::<Value>(&decoded).ok()?;
    parsed
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[derive(Debug, Deserialize)]
struct UsageErrorResponse {
    error: UsageErrorBody,
}

#[derive(Debug, Deserialize)]
struct UsageErrorBody {
    code: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
    plan_type: Option<PlanType>,
    resets_at: Option<i64>,
    limit_window_minutes: Option<Value>,
}
