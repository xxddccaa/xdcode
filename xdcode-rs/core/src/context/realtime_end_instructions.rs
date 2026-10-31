use super::ContextualUserFragment;
use xdcode_prompts::END_INSTRUCTIONS;
use xdcode_protocol::models::ContentItem;
use xdcode_protocol::models::ContentItemKind;
use xdcode_protocol::models::input_text;
use xdcode_protocol::protocol::REALTIME_CONVERSATION_CLOSE_TAG;
use xdcode_protocol::protocol::REALTIME_CONVERSATION_OPEN_TAG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RealtimeEndInstructions {
    instructions: Option<String>,
}

impl RealtimeEndInstructions {
    pub(crate) fn new() -> Self {
        Self { instructions: None }
    }

    pub(crate) fn with_instructions(instructions: impl Into<String>) -> Self {
        Self {
            instructions: Some(instructions.into()),
        }
    }
}

impl ContextualUserFragment for RealtimeEndInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("realtime_conversation.end_instructions".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            REALTIME_CONVERSATION_OPEN_TAG,
            REALTIME_CONVERSATION_CLOSE_TAG,
        )
    }

    fn body(&self) -> ContentItem {
        let instructions = self
            .instructions
            .as_deref()
            .unwrap_or_else(|| END_INSTRUCTIONS.trim());
        input_text(format!("\n{instructions}\n"))
    }
}
