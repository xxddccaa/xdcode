use super::ContextualUserFragment;
use xdcode_protocol::models::ContentItem;
use xdcode_protocol::models::ContentItemKind;
use xdcode_protocol::models::input_text;
use xdcode_protocol::protocol::REALTIME_CONVERSATION_CLOSE_TAG;
use xdcode_protocol::protocol::REALTIME_CONVERSATION_OPEN_TAG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RealtimeStartWithInstructions {
    instructions: String,
}

impl RealtimeStartWithInstructions {
    pub(crate) fn new(instructions: impl Into<String>) -> Self {
        Self {
            instructions: instructions.into(),
        }
    }
}

impl ContextualUserFragment for RealtimeStartWithInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("realtime_conversation.custom_start_instructions".to_string())
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
        input_text(format!("\n{}\n", self.instructions))
    }
}
