use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use xdcode_exec_server::EnvironmentManager;
use xdcode_exec_server::ExecServerRuntimeOptions;
use xdcode_extension_api::ExtensionRegistry;
use xdcode_extension_api::UserInstructionsProvider;
use xdcode_login::AuthManager;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::models::ResponseItem;
use xdcode_protocol::protocol::SessionSource;
use xdcode_protocol::user_input::UserInput;

use crate::config::Config;
use crate::resolve_installation_id;
use crate::session::session::Session;
use crate::session::turn::build_prompt;
use crate::state_db_bridge::StateDbHandle;
use crate::thread_manager::StartThreadOptions;
use crate::thread_manager::ThreadManager;
use crate::thread_manager::thread_store_from_config;

/// Build the model-visible `input` list for a single debug turn.
#[doc(hidden)]
pub async fn build_prompt_input(
    mut config: Config,
    input: Vec<UserInput>,
    state_db: Option<StateDbHandle>,
    extensions: Arc<ExtensionRegistry<Config>>,
    user_instructions_provider: Arc<dyn UserInstructionsProvider>,
) -> XdcodeResult<Vec<ResponseItem>> {
    config.ephemeral = true;

    let auth_manager =
        AuthManager::shared_from_config(&config, /*enable_xdcode_api_key_env*/ false)
            .await
            .map_err(|err| XdcodeErr::Fatal(err.to_string()))?;

    let local_runtime_paths = ExecServerRuntimeOptions::from_optional_paths(
        config.xdcode_self_exe.clone(),
        config.xdcode_linux_sandbox_exe.clone(),
    )?;

    let thread_store = thread_store_from_config(&config, state_db.clone());
    let installation_id = resolve_installation_id(&config.xdcode_home).await?;
    let thread_manager = ThreadManager::new(
        &config,
        Arc::clone(&auth_manager),
        crate::thread_manager::build_models_manager(&config, Arc::clone(&auth_manager)),
        crate::XdcodeAppsToolsCache::default(),
        SessionSource::Exec,
        Arc::new(
            EnvironmentManager::from_xdcode_home(
                config.xdcode_home.clone(),
                Some(local_runtime_paths),
                config.http_client_factory(),
            )
            .await
            .map_err(|err| XdcodeErr::Fatal(err.to_string()))?,
        ),
        extensions,
        user_instructions_provider,
        /*analytics_events_client*/ None,
        crate::thread_manager::passthrough_image_store(),
        thread_store,
        crate::local_agent_graph_store_from_state_db(state_db.as_ref()),
        installation_id,
        /*attestation_provider*/ None,
        /*external_time_provider*/ None,
    );
    let thread = thread_manager
        .start_thread(StartThreadOptions::new(config))
        .await?;

    let output = build_prompt_input_from_session(&thread.thread.session, input).await;
    let shutdown = thread.thread.shutdown_and_wait().await;
    let _removed = thread_manager.remove_thread(&thread.thread_id).await;

    shutdown?;
    output
}

pub(crate) async fn build_prompt_input_from_session(
    sess: &Arc<Session>,
    input: Vec<UserInput>,
) -> XdcodeResult<Vec<ResponseItem>> {
    let turn_context = sess.new_default_turn().await;
    // Prompt debugging builds a standalone request without entering run_turn.
    let step_context = sess
        .capture_step_context(Arc::clone(&turn_context), &CancellationToken::new())
        .await?;
    sess.record_context_updates_and_set_reference_context_item(step_context.as_ref())
        .await?;

    if !input.is_empty() {
        let response_item = sess.response_item_from_user_input(input);
        sess.record_conversation_items(
            turn_context.as_ref(),
            &step_context.settings.model_info,
            std::slice::from_ref(&response_item),
        )
        .await;
    }

    let prompt_input = sess
        .clone_history()
        .await
        .for_prompt(&step_context.settings.model_info.input_modalities);
    let base_instructions = sess.get_base_instructions().await;
    let prompt = build_prompt(
        prompt_input,
        step_context.as_ref(),
        base_instructions,
        sess.current_window_uses_incremental_tools(&step_context)
            .await,
    );

    Ok(prompt.input)
}
