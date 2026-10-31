use crate::StartThreadOptions;
use crate::ThreadManager;
use crate::agent::LocalAgentControl;
use crate::config::Config;
use crate::config::test_config;
use crate::thread_manager::ThreadManagerState;
use crate::xdcode_thread::XdcodeThread;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use xdcode_features::Feature;
use xdcode_login::XdcodeAuth;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::XdcodeErrorDetails;
use xdcode_protocol::protocol::EventMsg;
use xdcode_protocol::protocol::SessionSource;
use xdcode_protocol::protocol::SubAgentSource;
use xdcode_protocol::protocol::ThreadSource;
use xdcode_protocol::protocol::TurnAbortReason;
use xdcode_protocol::protocol::TurnAbortedEvent;
use xdcode_protocol::protocol::TurnCompleteEvent;

#[tokio::test]
async fn residency_slot_reservation_unloads_oldest_idle_v2_agent() {
    let mut config = test_config().await;
    let _ = config.features.enable(Feature::MultiAgentV2);
    config.multi_agent_v2.max_concurrent_threads_per_session = 2;
    let temp_home = tempfile::tempdir().expect("create temp home");
    config.xdcode_home = temp_home.path().to_path_buf().try_into().unwrap();
    config.cwd = temp_home.path().to_path_buf().try_into().unwrap();
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        XdcodeAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.xdcode_home.to_path_buf(),
        Arc::new(xdcode_exec_server::EnvironmentManager::default_for_tests()),
    );
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .expect("start root thread");
    let control = manager.agent_control();
    let state = control
        .runtime
        .upgrade()
        .expect("thread manager should be live");
    let membership = control.runtime.admit_start().expect("admit residency work");

    let first_slot = control
        .reserve_v2_residency_slot(
            &state,
            &config,
            &membership,
            /*protected_thread_id*/ None,
        )
        .await
        .expect("first resident slot");
    let first =
        spawn_v2_subagent(&control, &state, config.clone(), root.thread_id, "worker-1").await;
    first_slot.commit(first.thread_id);
    mark_thread_completed(first.thread.as_ref()).await;

    let second_slot = control
        .reserve_v2_residency_slot(
            &state,
            &config,
            &membership,
            /*protected_thread_id*/ None,
        )
        .await
        .expect("second resident slot should evict the first idle agent");
    match manager.get_thread(first.thread_id).await {
        Err(err) => match err.details() {
            XdcodeErrorDetails::ThreadNotFound(thread_id) => {
                assert_eq!(*thread_id, first.thread_id)
            }
            _ => panic!("expected evicted thread to be missing, got {err:?}"),
        },
        Ok(_) => panic!("expected evicted thread to be missing"),
    }
    let second = spawn_v2_subagent(&control, &state, config, root.thread_id, "worker-2").await;
    second_slot.commit(second.thread_id);

    assert!(manager.get_thread(root.thread_id).await.is_ok());
    assert!(manager.get_thread(second.thread_id).await.is_ok());
}

#[tokio::test]
async fn interrupted_v2_agent_is_lost_after_residency_eviction() {
    let mut config = test_config().await;
    let _ = config.features.enable(Feature::MultiAgentV2);
    config.multi_agent_v2.max_concurrent_threads_per_session = 2;
    let temp_home = tempfile::tempdir().expect("create temp home");
    config.xdcode_home = temp_home.path().to_path_buf().try_into().unwrap();
    config.cwd = temp_home.path().to_path_buf().try_into().unwrap();
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        XdcodeAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.xdcode_home.to_path_buf(),
        Arc::new(xdcode_exec_server::EnvironmentManager::default_for_tests()),
    );
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .expect("start root thread");
    let control = manager.agent_control();
    let state = control
        .runtime
        .upgrade()
        .expect("thread manager should be live");
    let membership = control.runtime.admit_start().expect("admit residency work");

    let first_slot = control
        .reserve_v2_residency_slot(
            &state,
            &config,
            &membership,
            /*protected_thread_id*/ None,
        )
        .await
        .expect("first resident slot");
    let first =
        spawn_v2_subagent(&control, &state, config.clone(), root.thread_id, "worker-1").await;
    first_slot.commit(first.thread_id);
    mark_thread_interrupted(first.thread.as_ref()).await;

    let second_slot = control
        .reserve_v2_residency_slot(
            &state,
            &config,
            &membership,
            /*protected_thread_id*/ None,
        )
        .await
        .expect("second resident slot should evict the first interrupted idle agent");
    match manager.get_thread(first.thread_id).await {
        Err(err) => match err.details() {
            XdcodeErrorDetails::ThreadNotFound(thread_id) => {
                assert_eq!(*thread_id, first.thread_id)
            }
            _ => panic!("expected evicted thread to be missing, got {err:?}"),
        },
        Ok(_) => panic!("expected evicted thread to be missing"),
    }
    let second =
        spawn_v2_subagent(&control, &state, config.clone(), root.thread_id, "worker-2").await;
    second_slot.commit(second.thread_id);
    mark_thread_completed(second.thread.as_ref()).await;

    let err = control
        .ensure_v2_agent_loaded(config, first.thread_id, /*parent*/ None)
        .await
        .expect_err("evicted interrupted agent should stay lost");
    match err.details() {
        XdcodeErrorDetails::ThreadNotFound(thread_id) => assert_eq!(*thread_id, first.thread_id),
        _ => panic!("expected ThreadNotFound, got {err:?}"),
    }

    assert!(manager.get_thread(root.thread_id).await.is_ok());
    assert!(manager.get_thread(second.thread_id).await.is_ok());
    match manager.get_thread(first.thread_id).await {
        Err(err) => match err.details() {
            XdcodeErrorDetails::ThreadNotFound(thread_id) => {
                assert_eq!(*thread_id, first.thread_id)
            }
            _ => panic!("expected evicted thread to be missing, got {err:?}"),
        },
        Ok(_) => panic!("expected evicted thread to be missing"),
    }
}

async fn spawn_v2_subagent(
    control: &LocalAgentControl,
    state: &Arc<ThreadManagerState>,
    config: Config,
    parent_thread_id: ThreadId,
    label: &str,
) -> crate::thread_manager::NewThread {
    state
        .spawn_child_thread(
            crate::thread_manager::StartThreadOptions {
                session_source: Some(SessionSource::SubAgent(SubAgentSource::Other(
                    label.to_string(),
                ))),
                thread_source: Some(ThreadSource::Subagent),
                ..crate::thread_manager::StartThreadOptions::new(config)
            },
            control.clone(),
            Some(parent_thread_id),
            /*inherited_exec_policy*/ None,
        )
        .await
        .expect("spawn v2 subagent")
}

async fn mark_thread_completed(thread: &XdcodeThread) {
    let turn = thread.session.new_default_turn().await;
    thread
        .session
        .send_event(
            turn.as_ref(),
            EventMsg::TurnComplete(TurnCompleteEvent {
                root_turn_id: None,
                turn_id: turn.sub_id.clone(),
                started_at: None,
                last_agent_message: Some("done".to_string()),
                error: None,
                completed_at: None,
                duration_ms: None,
                time_to_first_token_ms: None,
            }),
        )
        .await;
    clear_active_turn(thread).await;
}

async fn mark_thread_interrupted(thread: &XdcodeThread) {
    let turn = thread.session.new_default_turn().await;
    thread
        .session
        .send_event(
            turn.as_ref(),
            EventMsg::TurnAborted(TurnAbortedEvent {
                root_turn_id: None,
                turn_id: Some(turn.sub_id.clone()),
                started_at: None,
                reason: TurnAbortReason::Interrupted,
                error: None,
                completed_at: None,
                duration_ms: None,
            }),
        )
        .await;
    clear_active_turn(thread).await;
}

async fn clear_active_turn(thread: &XdcodeThread) {
    // The fixture has no task runner to clear the turn after the terminal event.
    *thread.session.active_turn.lock().await = None;
}
