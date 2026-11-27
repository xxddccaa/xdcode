//! Resolves the classifier's own model budget and transport configuration.
//! Parent-model context overrides do not change the classifier allowance.

use std::sync::Arc;

use super::sampler::LunaSamplerConfig;
use super::sampler::MODEL;
use xdcode_core::SharedModelsManager;
use xdcode_core::config::Config;
use xdcode_core::cyber_access_program::ApiKeyCyberAccessPrograms;
use xdcode_extension_api::ThreadOriginator;
use xdcode_extension_api::ThreadStartInput;
use xdcode_features::Feature;
use xdcode_login::AgentIdentityAuthPolicy;
use xdcode_login::AuthManager;

// Decisions setup failures are telemetry only; they never affect the baseline sampler.
pub(super) fn decisions_sampler(
    _input: &ThreadStartInput<'_, Config>,
    _auth_manager: &Arc<AuthManager>,
) -> Option<super::decisions::DecisionsSampler> {
    // The optional hosted comparison never participates in local approval
    // decisions. Keep the primary sampler and its custom provider unchanged.
    None
}

// Decisions uses a commercial endpoint. Reject padded keys without rewriting credentials.
pub(super) fn valid_implicit_decisions_key(key: &str) -> bool {
    !key.is_empty() && key.trim() == key && !xdcode_login::auth::is_fedramp_api_key(key)
}

pub(super) async fn sampler_config(input: &ThreadStartInput<'_, Config>) -> LunaSamplerConfig {
    let inherit_parent_cyber_access_program =
        xdcode_core::RequiredCyberAccessPrograms::for_thread(input.thread_store).is_none();
    let luna_model = if let Some(catalog) = input.thread_store.get::<SharedModelsManager>() {
        let mut model_config = input.config.to_models_manager_config();
        model_config.model_context_window = None;
        Some(catalog.get_model_info(MODEL, &model_config).await)
    } else {
        None
    };
    let max_input_tokens =
        luna_model
            .as_ref()
            .map_or(xdcode_guardian_context::DEFAULT_MAX_INPUT_TOKENS, |model| {
                xdcode_guardian_context::effective_input_token_limit(
                    model, /*configured_window*/ None,
                )
            });
    let luna_compaction_hash = luna_model.and_then(|model| model.comp_hash.clone());
    #[expect(clippy::expect_used, reason = "Required thread startup invariant")]
    let auth = input
        .thread_store
        .get::<xdcode_model_provider::ProviderAuthContext>()
        .expect("Core installs provider auth before starting Guardian");
    LunaSamplerConfig {
        api_key_cyber_access_programs: ApiKeyCyberAccessPrograms::from_config(input.config),
        inherit_parent_cyber_access_program,
        workspace_routing: input
            .thread_store
            .get_or_init(|| input.config.workspace_routing_context())
            .as_ref()
            .clone(),
        provider: auth.model_provider(input.config.model_provider.clone()),
        http_client_factory: input.config.http_client_factory(),
        agent_identity_policy: if input.config.features.enabled(Feature::UseAgentIdentity) {
            AgentIdentityAuthPolicy::ChatGptAuth
        } else {
            AgentIdentityAuthPolicy::JwtOnly
        },
        session_source: input.session_source.clone(),
        session_id: input.session_store.level_id().to_string(),
        thread_id: input.thread_store.level_id().to_string(),
        originator: input
            .thread_store
            .get::<ThreadOriginator>()
            .map(|originator| originator.0.clone()),
        service_tier: input.config.service_tier.clone(),
        luna_compaction_hash,
        max_input_tokens,
        metrics: input.extension_metrics.clone(),
    }
}
