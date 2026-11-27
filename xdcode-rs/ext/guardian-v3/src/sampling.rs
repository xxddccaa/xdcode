//! Encode a whole Guardian input within the model's context budget.

use serde::Serialize;
use xdcode_protocol::models::FunctionCallOutputPayload;
use xdcode_protocol::models::ResponseItem;
use xdcode_protocol::protocol::TruncationPolicy;
use xdcode_utils_string::to_json_string_bounded;

pub(crate) fn context_input(
    name: &str,
    input: &impl Serialize,
    policy: TruncationPolicy,
) -> anyhow::Result<ResponseItem> {
    Ok(ResponseItem::FunctionCallOutput {
        id: None,
        call_id: None,
        name: Some(name.into()),
        namespace: Some("guardian".into()),
        output: FunctionCallOutputPayload::from_text(to_json_string_bounded(
            input,
            policy.byte_budget(),
        )?),
        internal_chat_message_metadata_passthrough: None,
    })
}
