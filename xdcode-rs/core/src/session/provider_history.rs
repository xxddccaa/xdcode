//! Provider attribution for restored model history. Shared listings do not grant
//! permission to forward provider-owned ciphertext to another endpoint.

use crate::context_manager::ContextManager;
use std::collections::HashMap;
use xdcode_history::InitialHistory;
use xdcode_history::RolloutItem;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::models::ResponseItem;
use xdcode_protocol::protocol::EventMsg;
use xdcode_protocol::protocol::ThreadHistoryMode;
use xdcode_protocol::protocol::TruncationPolicy;

/// Attribute opaque tokens chronologically, then use actual rollback/compaction
/// replay to check only the history that can reach the next model request.
/// A durable TurnContext pins attribution even if /model changes thread defaults
/// while the original turn is still emitting output. Unknown legacy ownership
/// is safe only when that content is portable or has been retired by compaction.
pub(super) fn validate_initial_history(
    history: &InitialHistory,
    target_provider: &str,
    target_fingerprint: &str,
) -> XdcodeResult<()> {
    let mut source_provider: Option<&str> = None;
    let mut source_fingerprint: Option<&str> = None;
    let mut turn_pinned = false;
    let mut token_ownership = HashMap::<String, bool>::new();
    for item in history.get_rollout_items() {
        let owned = source_provider == Some(target_provider)
            && source_fingerprint == Some(target_fingerprint);
        let mut record = |item: &ResponseItem| -> XdcodeResult<()> {
            for token in opaque_tokens(item)? {
                token_ownership
                    .entry(token)
                    .and_modify(|known| *known &= owned)
                    .or_insert(owned);
            }
            Ok(())
        };
        match item {
            RolloutItem::SessionMeta(meta) => {
                source_provider = meta.meta.model_provider.as_deref();
                source_fingerprint = None;
                turn_pinned = false;
            }
            RolloutItem::TurnContext(context) => {
                source_provider = context.model_provider_id.as_deref();
                source_fingerprint = context.provider_fingerprint.as_deref();
                turn_pinned = true;
            }
            RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(event)) if !turn_pinned => {
                source_provider = Some(&event.thread_settings.model_provider_id);
                source_fingerprint = event.thread_settings.provider_fingerprint.as_deref();
            }
            RolloutItem::ResponseItem(envelope) => record(&envelope.item)?,
            RolloutItem::Compacted(compacted) => {
                if let Some(replacement) = &compacted.replacement_history {
                    for envelope in replacement {
                        record(&envelope.item)?;
                    }
                }
            }
            _ => {}
        }
    }
    let active = ContextManager::reconstruct_rollout(
        history.get_rollout_items(),
        history.get_history_mode(ThreadHistoryMode::default()),
        ContextManager::new(),
        TruncationPolicy::Bytes(usize::MAX / 2),
    );
    for envelope in active.history {
        for token in opaque_tokens(&envelope.item)? {
            if token_ownership.get(&token) != Some(&true) {
                return Err(boundary_error(target_provider));
            }
        }
    }
    Ok(())
}

fn opaque_tokens(item: &ResponseItem) -> XdcodeResult<Vec<String>> {
    fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Array(values) => {
                for value in values {
                    collect(value, out);
                }
            }
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if (key == "encrypted_content" || key == "encrypted_function_args")
                        && !value.is_null()
                        && value.as_str().is_none_or(|value| !value.is_empty())
                    {
                        out.push(value.to_string());
                    } else {
                        collect(value, out);
                    }
                }
            }
            _ => {}
        }
    }
    let value = serde_json::to_value(item).map_err(|_| {
        XdcodeErr::InvalidRequest("unable to validate restored provider ownership".into())
    })?;
    let mut tokens = Vec::new();
    collect(&value, &mut tokens);
    Ok(tokens)
}

fn boundary_error(target: &str) -> XdcodeErr {
    XdcodeErr::InvalidRequest(format!(
        "Cannot verify endpoint `{target}` owns the provider-specific encrypted history. Resume with its original provider or create a new thread; the saved history has been preserved."
    ))
}

#[cfg(test)]
#[path = "provider_history_tests.rs"]
mod tests;
