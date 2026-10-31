//! Test-only helpers exposed for cross-crate integration tests.
//!
//! Production code should not depend on this module.
//! We prefer this to using a crate feature to avoid building multiple
//! permutations of the crate.

use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::Lazy;
use xdcode_exec_server::EnvironmentManager;
use xdcode_extension_api::LoadInstructionsFuture;
use xdcode_extension_api::LoadedUserInstructions;
use xdcode_extension_api::UserInstructionsProvider;
use xdcode_http_client::HttpClientFactory;
use xdcode_http_client::OutboundProxyPolicy;
use xdcode_login::AuthManager;
use xdcode_login::XdcodeAuth;
use xdcode_model_provider::create_model_provider;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_models_manager::bundled_models_response;
use xdcode_models_manager::collaboration_mode_presets;
use xdcode_models_manager::manager::SharedModelsManager;
use xdcode_models_manager::test_support::construct_model_info_offline_for_tests;
use xdcode_models_manager::test_support::get_model_offline_for_tests;
use xdcode_protocol::ThreadId;
use xdcode_protocol::config_types::CollaborationModeMask;
use xdcode_protocol::mcp::ClientMcpExtensions;
use xdcode_protocol::mcp::PROVIDER_FORM_EXTENSION_ID;
use xdcode_protocol::model_types::ModelInfo;
use xdcode_protocol::model_types::ModelPreset;
use xdcode_protocol::protocol::SessionSource;

use crate::ThreadManager;
use crate::config::Config;
use crate::responses_metadata::XdcodeResponsesMetadata;
use crate::responses_metadata::XdcodeResponsesRequestKind;
use crate::responses_metadata::subagent_header_value;
use crate::responses_metadata::subagent_metadata_kind;
use crate::thread_manager;
use crate::unified_exec;

pub fn custom_provider(base_url: String) -> ModelProviderInfo {
    let mut provider = ModelProviderInfo::create_custom_provider(Some(base_url));
    provider.env_key = None;
    provider.experimental_bearer_token = Some("xdcode-test-credential".into());
    provider
}

pub fn custom_provider_config() -> xdcode_config::config_toml::ConfigToml {
    xdcode_config::config_toml::ConfigToml {
        model: Some("gpt-5.5".to_string()),
        model_provider: Some("test-provider".to_string()),
        model_providers: std::collections::HashMap::from([(
            "test-provider".to_string(),
            custom_provider("http://127.0.0.1:9/v1".to_string()),
        )]),
        ..Default::default()
    }
}

static TEST_MODEL_PRESETS: Lazy<Vec<ModelPreset>> = Lazy::new(|| {
    let mut response = bundled_models_response()
        .unwrap_or_else(|err| panic!("bundled models.json should parse: {err}"));
    response.models.sort_by_key(|model| model.priority);
    let mut presets: Vec<ModelPreset> = response.models.into_iter().map(Into::into).collect();
    ModelPreset::mark_default_by_picker_visibility(&mut presets);
    presets
});

/// Inspect the same resolved environment configurations used to construct sampling steps.
pub async fn environment_windows_sandbox_types(
    thread: &crate::XdcodeThread,
) -> Vec<(String, xdcode_sandboxing::SandboxType)> {
    thread
        .session
        .services
        .turn_environments
        .snapshot()
        .await
        .turn_environments()
        .map(|environment| {
            (
                environment.selection.environment_id.clone(),
                environment.config().windows_sandbox_type,
            )
        })
        .collect()
}

/// Reattaches request-only observations to a completed turn's history for capture assertions.
/// Tests inspect this separately from the destination-filtered HTTP/WS request.
pub async fn history_with_tool_call_metadata(
    thread: &crate::XdcodeThread,
) -> Vec<xdcode_protocol::models::ResponseItem> {
    let history = thread.conversation_history_snapshot().await;
    let mut items = history.items().cloned().collect::<Vec<_>>();
    thread
        .session
        .services
        .executed_tool_calls
        .attach_to_prompt(&mut items, &mut Default::default());
    items
}

/// Returns the recorder state used by the next session metadata snapshot.
/// This does not change destination filtering or issue a request.
pub fn mcp_attribution_snapshot(
    thread: &crate::XdcodeThread,
) -> xdcode_protocol::mcp::McpAttribution {
    thread
        .session
        .services
        .executed_tool_calls
        .mcp_attribution_snapshot()
}

/// Test-only provider that supplies no user instructions.
#[derive(Debug, Default)]
pub struct EmptyUserInstructionsProvider;

impl UserInstructionsProvider for EmptyUserInstructionsProvider {
    fn load_user_instructions(&self) -> LoadInstructionsFuture<'_> {
        Box::pin(async { LoadedUserInstructions::default() })
    }
}

pub fn set_thread_manager_test_mode(enabled: bool) {
    thread_manager::set_thread_manager_test_mode_for_tests(enabled);
}

pub fn set_deterministic_process_ids(enabled: bool) {
    unified_exec::set_deterministic_process_ids_for_tests(enabled);
}

pub fn auth_manager_from_auth(auth: XdcodeAuth) -> Arc<AuthManager> {
    AuthManager::from_auth_for_testing(auth)
}

pub fn auth_manager_from_auth_with_home(
    auth: XdcodeAuth,
    xdcode_home: PathBuf,
) -> Arc<AuthManager> {
    AuthManager::from_auth_for_testing_with_home(auth, xdcode_home)
}

pub fn with_code_mode_host_program(
    thread_manager: ThreadManager,
    host_program: PathBuf,
    config: &crate::config::Config,
) -> ThreadManager {
    thread_manager.with_code_mode_host_program_for_tests(host_program, config)
}

pub fn thread_manager_with_models_provider(
    auth: XdcodeAuth,
    provider: ModelProviderInfo,
) -> ThreadManager {
    ThreadManager::with_models_provider_for_tests(auth, provider)
}

pub fn thread_manager_with_models_provider_and_home(
    auth: XdcodeAuth,
    provider: ModelProviderInfo,
    xdcode_home: PathBuf,
    environment_manager: Arc<EnvironmentManager>,
) -> ThreadManager {
    ThreadManager::with_models_provider_and_home_for_tests(
        auth,
        provider,
        xdcode_home,
        environment_manager,
    )
}

pub async fn start_thread_with_user_shell_override(
    thread_manager: &ThreadManager,
    config: Config,
    user_shell_override: crate::shell::Shell,
    supports_provider_form_elicitation: bool,
) -> xdcode_protocol::error::Result<crate::NewThread> {
    thread_manager
        .start_thread_with_user_shell_override_for_tests(
            config,
            user_shell_override,
            ClientMcpExtensions::new(supports_provider_form_elicitation.then(|| {
                (
                    PROVIDER_FORM_EXTENSION_ID.to_string(),
                    serde_json::json!({}),
                )
            })),
        )
        .await
}

pub async fn resume_legacy_thread_from_rollout_with_user_shell_override(
    thread_manager: &ThreadManager,
    config: Config,
    rollout_path: PathBuf,
    auth_manager: Arc<AuthManager>,
    user_shell_override: crate::shell::Shell,
    supports_provider_form_elicitation: bool,
) -> xdcode_protocol::error::Result<crate::NewThread> {
    thread_manager
        .resume_legacy_thread_from_rollout_with_user_shell_override_for_tests(
            config,
            rollout_path,
            auth_manager,
            user_shell_override,
            ClientMcpExtensions::new(supports_provider_form_elicitation.then(|| {
                (
                    PROVIDER_FORM_EXTENSION_ID.to_string(),
                    serde_json::json!({}),
                )
            })),
        )
        .await
}

pub fn models_manager_with_provider(
    xdcode_home: PathBuf,
    auth_manager: Arc<AuthManager>,
    provider: ModelProviderInfo,
) -> SharedModelsManager {
    let provider = create_model_provider(provider, Some(auth_manager));
    provider.models_manager(xdcode_home, /*config_model_catalog*/ None)
}

pub fn default_http_client_factory() -> HttpClientFactory {
    HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
}

pub fn get_model_offline(model: Option<&str>) -> String {
    get_model_offline_for_tests(model)
}

pub fn construct_model_info_offline(model: &str, config: &Config) -> ModelInfo {
    construct_model_info_offline_for_tests(model, &config.to_models_manager_config())
}

#[derive(Clone, Copy)]
pub enum TestXdcodeResponsesRequestKind {
    Turn,
    Prewarm,
    WebsocketConnection,
}

#[allow(clippy::too_many_arguments)]
pub fn responses_metadata(
    installation_id: &str,
    session_id: &str,
    thread_id: &str,
    turn_id: Option<&str>,
    window_id: String,
    session_source: &SessionSource,
    parent_thread_id: Option<ThreadId>,
    request_kind: TestXdcodeResponsesRequestKind,
) -> XdcodeResponsesMetadata {
    let request_kind = match request_kind {
        TestXdcodeResponsesRequestKind::Turn => Some(XdcodeResponsesRequestKind::Turn),
        TestXdcodeResponsesRequestKind::Prewarm => Some(XdcodeResponsesRequestKind::Prewarm),
        TestXdcodeResponsesRequestKind::WebsocketConnection => None,
    };
    XdcodeResponsesMetadata {
        turn_id: request_kind.and(turn_id.map(ToString::to_string)),
        request_kind,
        parent_thread_id,
        subagent_header: subagent_header_value(session_source),
        subagent_kind: request_kind.and_then(|_| subagent_metadata_kind(session_source)),
        ..XdcodeResponsesMetadata::new(
            installation_id.to_string(),
            session_id.to_string(),
            thread_id.to_string(),
            window_id,
        )
    }
}

pub fn with_parent_turn(
    mut metadata: XdcodeResponsesMetadata,
    id: &str,
) -> XdcodeResponsesMetadata {
    metadata.parent_turn_id = Some(id.to_string());
    metadata
}

pub fn all_model_presets() -> &'static Vec<ModelPreset> {
    &TEST_MODEL_PRESETS
}

pub fn builtin_collaboration_mode_presets() -> Vec<CollaborationModeMask> {
    collaboration_mode_presets::builtin_collaboration_mode_presets()
}
