use crate::error::ApiError;
use futures::Stream;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use xdcode_protocol::ResponseUsageMetadata;
use xdcode_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use xdcode_protocol::config_types::Verbosity as VerbosityConfig;
use xdcode_protocol::model_types::ReasoningEffort as ReasoningEffortConfig;
use xdcode_protocol::models::ResponseItem;
use xdcode_protocol::protocol::ModelVerification;
use xdcode_protocol::protocol::RateLimitSnapshot;
use xdcode_protocol::protocol::TokenUsage;
use xdcode_protocol::protocol::TurnModerationMetadataEvent;
use xdcode_protocol::protocol::W3cTraceContext;
use xdcode_protocol::turn_input::CyberAccessProgram;

pub const WS_REQUEST_HEADER_TRACEPARENT_CLIENT_METADATA_KEY: &str = "ws_request_header_traceparent";
pub const WS_REQUEST_HEADER_TRACESTATE_CLIENT_METADATA_KEY: &str = "ws_request_header_tracestate";

/// Explicit per-request access selection using the Responses API wire values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AccessPrograms {
    cyber: &'static str,
}

impl From<CyberAccessProgram> for AccessPrograms {
    fn from(program: CyberAccessProgram) -> Self {
        Self {
            cyber: match program {
                CyberAccessProgram::Standard => "standard",
                CyberAccessProgram::DaybreakBlue => "daybreak_blue",
                CyberAccessProgram::DaybreakRed => "daybreak_red",
            },
        }
    }
}

/// Canonical input payload for the memory summarize endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct MemorySummarizeInput {
    pub model: String,
    #[serde(rename = "traces")]
    pub raw_memories: Vec<RawMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMemory {
    pub id: String,
    pub metadata: RawMemoryMetadata,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMemoryMetadata {
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MemorySummarizeOutput {
    #[serde(rename = "trace_summary", alias = "raw_memory")]
    pub raw_memory: String,
    pub memory_summary: String,
}

/// The latest server response ID received in this turn, shared with tool-review extensions.
#[derive(Clone, Debug)]
pub struct ResponseId(pub String);

#[derive(Debug)]
pub enum ResponseEvent {
    Created {
        /// Existing server response ID, when supplied by the stream.
        response_id: Option<String>,
    },
    SafetyBuffering(SafetyBuffering),
    OutputItemDone(ResponseItem),
    OutputItemAdded(ResponseItem),
    /// Emitted when the server includes `OpenAI-Model` on the stream response.
    /// This can differ from the requested model when backend safety routing applies.
    ServerModel(String),
    /// Emitted when the server recommends additional account verification.
    ModelVerifications(Vec<ModelVerification>),
    /// Emitted when the server includes moderation metadata for first-party turn presentation.
    TurnModerationMetadata(TurnModerationMetadataEvent),
    /// Emitted when `X-Reasoning-Included: true` is present on the response,
    /// meaning the server already accounted for past reasoning tokens and the
    /// client should not re-estimate them.
    ServerReasoningIncluded(bool),
    Completed {
        response_id: String,
        token_usage: Option<TokenUsage>,
        usage_metadata: Option<ResponseUsageMetadata>,
        /// Did the model affirmatively end its turn? Some providers do not set this,
        /// so we rely on fallback logic when this is `None`.
        end_turn: Option<bool>,
    },
    OutputTextDelta(String),
    ToolCallInputDelta {
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    ReasoningSummaryDelta {
        delta: String,
        summary_index: i64,
    },
    ReasoningSummaryDone {
        item_id: String,
        text: String,
        summary_index: i64,
    },
    ReasoningContentDelta {
        delta: String,
        content_index: i64,
    },
    ReasoningSummaryPartAdded {
        summary_index: i64,
    },
    RateLimits(RateLimitSnapshot),
    ModelsEtag(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SafetyBuffering {
    pub use_cases: Vec<String>,
    pub reasons: Vec<String>,
    #[serde(skip)]
    pub show_buffering_ui: bool,
    #[serde(rename = "retry_model")]
    pub faster_model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SafetyBufferingTreatment {
    pub faster_model: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningContext {
    Auto,
    CurrentTurn,
    AllTurns,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reasoning {
    pub effort: Option<ReasoningEffortConfig>,
    pub summary: Option<ReasoningSummaryConfig>,
    pub context: Option<ReasoningContext>,
    /// Preserve catalog effort strings exactly, including numeric-looking values.
    /// This transport setting is never included in the request body.
    pub preserve_effort_string: bool,
}

impl Serialize for Reasoning {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let field_count = usize::from(self.effort.is_some())
            + usize::from(self.summary.is_some())
            + usize::from(self.context.is_some());
        let mut state = serializer.serialize_struct("Reasoning", field_count)?;
        if let Some(effort) = &self.effort {
            state.serialize_field(
                "effort",
                &ReasoningEffortWire {
                    effort,
                    preserve_string: self.preserve_effort_string,
                },
            )?;
        }
        if let Some(summary) = &self.summary {
            state.serialize_field("summary", summary)?;
        }
        if let Some(context) = &self.context {
            state.serialize_field("context", context)?;
        }
        state.end()
    }
}

struct ReasoningEffortWire<'a> {
    effort: &'a ReasoningEffortConfig,
    preserve_string: bool,
}

impl Serialize for ReasoningEffortWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if !self.preserve_string
            && let ReasoningEffortConfig::Custom(value) = self.effort
            && let Ok(value) = value.parse::<u64>()
        {
            return serializer.serialize_u64(value);
        }
        self.effort.serialize(serializer)
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSummaryDelivery {
    SequentialCutoff,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct StreamOptions {
    pub reasoning_summary_delivery: ReasoningSummaryDelivery,
}

#[derive(Debug, Serialize, Default, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextFormatType {
    #[default]
    JsonSchema,
}

#[derive(Debug, Serialize, Default, Clone, PartialEq)]
pub struct TextFormat {
    /// Format type used by the Provider text controls.
    pub r#type: TextFormatType,
    /// When true, the server is expected to strictly validate responses.
    pub strict: bool,
    /// JSON schema for the desired output.
    pub schema: Value,
    /// Friendly name for the format, used in telemetry/debugging.
    pub name: String,
}

/// Controls the `text` field for the Responses API, combining verbosity and
/// optional JSON schema output formatting.
#[derive(Debug, Serialize, Default, Clone, PartialEq)]
pub struct TextControls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<ProviderVerbosity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
}

#[derive(Debug, Serialize, Default, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderVerbosity {
    Low,
    #[default]
    Medium,
    High,
}

impl From<VerbosityConfig> for ProviderVerbosity {
    fn from(v: VerbosityConfig) -> Self {
        match v {
            VerbosityConfig::Low => ProviderVerbosity::Low,
            VerbosityConfig::Medium => ProviderVerbosity::Medium,
            VerbosityConfig::High => ProviderVerbosity::High,
        }
    }
}

/// Serialized tool definitions for Responses API requests.
///
/// Keeping the tool list as raw JSON avoids rebuilding a generic JSON value
/// tree, while the shared allocation keeps request clones cheap.
#[derive(Debug, Clone)]
pub struct ResponsesApiTools(Arc<RawValue>);

impl ResponsesApiTools {
    pub(crate) fn as_raw_value(&self) -> &RawValue {
        &self.0
    }
}

impl From<Arc<RawValue>> for ResponsesApiTools {
    fn from(value: Arc<RawValue>) -> Self {
        Self(value)
    }
}

impl PartialEq for ResponsesApiTools {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.get() == other.0.get()
    }
}

impl Serialize for ResponsesApiTools {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ResponsesApiRequest {
    // Keep routing fields first: serde serializes struct fields in declaration order, and
    // gateways may inspect request bodies incrementally before potentially multi-megabyte input.
    pub model: String,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    pub input: Vec<ResponseItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ResponsesApiTools>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<Reasoning>,
    pub store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    pub include: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextControls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_programs: Option<AccessPrograms>,
}

impl<'a> From<&'a ResponsesApiRequest> for ResponseCreateWsRequest<'a> {
    fn from(request: &'a ResponsesApiRequest) -> Self {
        Self {
            model: &request.model,
            previous_response_id: None,
            input: &request.input,
            tools: request.tools.as_ref().map(ResponsesApiTools::as_raw_value),
            tool_choice: &request.tool_choice,
            parallel_tool_calls: request.parallel_tool_calls,
            reasoning: request.reasoning.as_ref(),
            store: request.store,
            stream: request.stream,
            stream_options: request.stream_options.as_ref(),
            include: &request.include,
            service_tier: request.service_tier.as_deref(),
            prompt_cache_key: request.prompt_cache_key.as_deref(),
            text: request.text.as_ref(),
            generate: None,
            client_metadata: request.client_metadata.clone(),
            access_programs: request.access_programs,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponseCreateWsRequest<'a> {
    // Keep routing fields first: serde serializes struct fields in declaration order, and
    // gateways may inspect request bodies incrementally before potentially multi-megabyte input.
    pub model: &'a str,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub input: &'a [ResponseItem],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<&'a RawValue>,
    pub tool_choice: &'a str,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<&'a Reasoning>,
    pub store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<&'a StreamOptions>,
    pub include: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a TextControls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_programs: Option<AccessPrograms>,
}

pub fn response_create_client_metadata(
    client_metadata: Option<HashMap<String, String>>,
    trace: Option<&W3cTraceContext>,
) -> Option<HashMap<String, String>> {
    let mut client_metadata = client_metadata.unwrap_or_default();

    if let Some(traceparent) = trace.and_then(|trace| trace.traceparent.as_deref()) {
        client_metadata.insert(
            WS_REQUEST_HEADER_TRACEPARENT_CLIENT_METADATA_KEY.to_string(),
            traceparent.to_string(),
        );
    }
    if let Some(tracestate) = trace.and_then(|trace| trace.tracestate.as_deref()) {
        client_metadata.insert(
            WS_REQUEST_HEADER_TRACESTATE_CLIENT_METADATA_KEY.to_string(),
            tracestate.to_string(),
        );
    }

    (!client_metadata.is_empty()).then_some(client_metadata)
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum ResponsesWsRequest<'a> {
    #[serde(rename = "response.create")]
    ResponseCreate(ResponseCreateWsRequest<'a>),
}

pub fn create_text_param_for_request(
    verbosity: Option<VerbosityConfig>,
    output_schema: &Option<Value>,
    output_schema_strict: bool,
) -> Option<TextControls> {
    if verbosity.is_none() && output_schema.is_none() {
        return None;
    }

    Some(TextControls {
        verbosity: verbosity.map(std::convert::Into::into),
        format: output_schema.as_ref().map(|schema| TextFormat {
            r#type: TextFormatType::JsonSchema,
            strict: output_schema_strict,
            schema: schema.clone(),
            name: "xdcode_output_schema".to_string(),
        }),
    })
}

/// Distinguishes draining a live response from abandoning obsolete work.
#[derive(Clone, Copy)]
pub enum ResponseInterrupt {
    /// Drain normally, without validating or waiting for interruption acknowledgements.
    Drain,
    /// Skip undispatched work or retire its transport at the cleanup deadline.
    /// Undispatched work ends without a completion event and leaves the socket reusable.
    Cancel { deadline: tokio::time::Instant },
}

pub struct ResponseStream {
    pub rx_event: mpsc::Receiver<Result<ResponseEvent, ApiError>>,
    /// Server-assigned `x-request-id` response header, when present.
    pub upstream_request_id: Option<String>,
    /// Requests a graceful interrupt. Keep consuming events through completion.
    /// Only `Cancel` validates and waits for an interruption acknowledgement.
    pub interrupt: Option<oneshot::Sender<ResponseInterrupt>>,
}

impl Stream for ResponseStream {
    type Item = Result<ResponseEvent, ApiError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx_event.poll_recv(cx)
    }
}

#[cfg(test)]
mod reasoning_wire_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_efforts_remain_exact_strings_on_the_wire() {
        for effort in ["001", "1234", "provider-custom-level", "none"] {
            let reasoning = Reasoning {
                effort: Some(effort.parse().unwrap()),
                summary: None,
                context: None,
                preserve_effort_string: true,
            };
            assert_eq!(
                serde_json::to_value(reasoning).unwrap(),
                json!({ "effort": effort })
            );
        }
    }

    #[test]
    fn service_default_omits_effort_and_retains_summary_and_context() {
        let reasoning = Reasoning {
            effort: None,
            summary: Some(ReasoningSummaryConfig::Detailed),
            context: Some(ReasoningContext::AllTurns),
            preserve_effort_string: true,
        };
        assert_eq!(
            serde_json::to_value(reasoning).unwrap(),
            json!({ "summary": "detailed", "context": "all_turns" })
        );

        let empty = Reasoning {
            effort: None,
            summary: None,
            context: None,
            preserve_effort_string: true,
        };
        assert_eq!(serde_json::to_value(empty).unwrap(), json!({}));
    }

    #[test]
    fn legacy_efforts_keep_numeric_serialization() {
        for (effort, expected) in [
            ("001", json!(1)),
            ("1234", json!(1234)),
            ("provider-custom-level", json!("provider-custom-level")),
            ("none", json!("none")),
        ] {
            let reasoning = Reasoning {
                effort: Some(effort.parse().unwrap()),
                summary: None,
                context: None,
                preserve_effort_string: false,
            };
            assert_eq!(
                serde_json::to_value(reasoning).unwrap(),
                json!({ "effort": expected })
            );
        }
    }
}
