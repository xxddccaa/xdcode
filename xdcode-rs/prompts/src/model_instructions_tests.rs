//! Covers literal instruction templates and missing-versus-empty inputs.

use super::*;
use pretty_assertions::assert_eq;
use xdcode_protocol::model_types::ConfigShellToolType;
use xdcode_protocol::model_types::ModelMessages;
use xdcode_protocol::model_types::ModelVisibility;
use xdcode_protocol::model_types::TruncationPolicyConfig;

fn test_model(model_messages: Option<ModelMessages>) -> ModelInfo {
    ModelInfo {
        slug: "test-model".to_string(),
        display_name: "Test Model".to_string(),
        description: None,
        default_reasoning_level: None,
        supported_reasoning_levels: vec![],
        shell_type: ConfigShellToolType::UnifiedExec,
        visibility: ModelVisibility::List,
        supported_in_api: true,
        priority: 1,
        additional_speed_tiers: vec![],
        service_tiers: vec![],
        default_service_tier: None,
        available_access_programs: None,
        availability_nux: None,
        upgrade: None,
        model_messages,
        include_skills_usage_instructions: false,
        include_plugin_usage_instructions: false,
        include_apps_usage_instructions: false,
        supports_reasoning_summary_parameter: true,
        default_reasoning_summary: Default::default(),
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        web_search_tool_type: Default::default(),
        truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
        supports_image_detail_original: false,
        context_window: None,
        max_context_window: None,
        auto_compact_token_limit: None,
        comp_hash: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: vec![],
        input_modalities: vec![],
        used_fallback_model_metadata: false,
        supports_search_tool: false,
        supports_experimental_context: false,
        use_responses_lite: false,
        supports_reasoning_effort_updates: false,
        guardian: None,
        node_repl_auto_review_required: false,
        node_repl_disabled: false,
        auto_review_model_override: None,
        model_specialty: None,
        tool_mode: None,
        multi_agent_version: None,
        multi_agent_reasoning_effort: None,
    }
}

#[test]
fn preserves_model_template_and_appends_application_identity() {
    let template = "Hello {{ personality }}\n\n# Personality\n\nFixed instructions";
    let model = test_model(Some(ModelMessages {
        instructions_template: Some(template.to_string()),
        ..Default::default()
    }));
    assert_eq!(
        (
            ResolvedModelMessages::from_model(&model).instructions_template(),
            render_model_instructions(&model)
        ),
        (
            Some(template),
            format!("{template}\n\n{}", APPLICATION_IDENTITY.trim_end())
        ),
    );
}

#[test]
fn missing_and_empty_templates_keep_application_identity_and_retain_presence() {
    for (messages, template) in [
        (None, None),
        (Some(ModelMessages::default()), None),
        (
            Some(ModelMessages {
                instructions_template: Some(String::new()),
                ..Default::default()
            }),
            Some(""),
        ),
    ] {
        let model = test_model(messages);
        assert_eq!(
            (
                render_model_instructions(&model),
                ResolvedModelMessages::from_model(&model).instructions_template()
            ),
            (APPLICATION_IDENTITY.trim_end().to_owned(), template),
        );
    }
}

#[test]
fn identity_is_independent_of_model_catalog_identity() {
    let model = test_model(Some(ModelMessages {
        instructions_template: Some("You are an unrelated model assistant.".to_string()),
        ..Default::default()
    }));
    let rendered = render_model_instructions(&model);
    assert!(rendered.ends_with(APPLICATION_IDENTITY.trim_end()));
    assert!(rendered.contains("identify yourself as xdcode"));
}
