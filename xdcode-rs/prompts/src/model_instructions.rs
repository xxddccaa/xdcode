//! Combines captured model instructions with the stable application identity.

use crate::ResolvedModelMessages;
use xdcode_protocol::model_types::ModelInfo;

const APPLICATION_IDENTITY: &str = include_str!("../templates/identity.md");

/// Keep application identity stable across model selections and empty catalogs.
pub fn render_model_instructions(model_info: &ModelInfo) -> String {
    let template = ResolvedModelMessages::from_model(model_info).instructions_template();
    if template.is_none() {
        tracing::warn!(
            model = %model_info.slug,
            "Model has no instruction template; using application identity."
        );
    }
    match template {
        Some(template) if !template.is_empty() => {
            format!("{template}\n\n{}", APPLICATION_IDENTITY.trim_end())
        }
        _ => APPLICATION_IDENTITY.trim_end().to_owned(),
    }
}

#[cfg(test)]
#[path = "model_instructions_tests.rs"]
mod tests;
