use super::ContextualUserFragment;
use xdcode_protocol::models::ContentItem;
use xdcode_protocol::models::ContentItemKind;
use xdcode_protocol::models::input_text;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginInstructions {
    text: String,
}

impl PluginInstructions {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl ContextualUserFragment for PluginInstructions {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("plugins.instructions".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> ContentItem {
        input_text(self.text.clone())
    }
}
