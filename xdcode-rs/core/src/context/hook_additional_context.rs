use super::ContextualUserFragment;
use xdcode_hooks::HookContext;
use xdcode_protocol::models::ContentItem;
use xdcode_protocol::models::ContentItemKind;
use xdcode_protocol::models::ContentItemMetadata;
use xdcode_protocol::models::input_text;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HookAdditionalContext {
    context: HookContext,
}

impl HookAdditionalContext {
    pub(crate) fn new(context: HookContext) -> Self {
        Self { context }
    }
}

impl ContextualUserFragment for HookAdditionalContext {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("hooks.additional_context".to_string())
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
        input_text(self.context.text.clone())
    }

    fn content_metadata(&self) -> ContentItemMetadata {
        self.context.metadata.clone()
    }
}
