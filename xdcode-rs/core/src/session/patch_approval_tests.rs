//! File change approval replies are scoped to their requesting turn.

use super::*;
use pretty_assertions::assert_eq;

async fn make_thread_for_session(
    session: Arc<Session>,
    turn_context: &TurnContext,
    events: async_channel::Receiver<Event>,
    session_source: SessionSource,
) -> (crate::XdcodeThread, async_channel::Receiver<Submission>) {
    let rollout_path = session.current_rollout_path().await.expect("rollout path");
    let configured = SessionConfiguredEvent {
        session_id: session.session_id(),
        thread_id: session.thread_id(),
        forked_from_id: None,
        parent_thread_id: None,
        thread_source: None,
        thread_name: None,
        model: turn_context.model_info().slug.clone(),
        model_provider_id: turn_context.config.model_provider_id.clone(),
        service_tier: None,
        approval_policy: AskForApproval::Never,
        approvals_reviewer: xdcode_config::types::ApprovalsReviewer::User,
        permission_profile: turn_context.permission_profile(),
        active_permission_profile: None,
        cwd: turn_context.config.cwd.clone(),
        reasoning_effort: None,
        network_proxy: None,
        rollout_path: rollout_path.clone(),
    };
    let (submissions, receiver) = async_channel::bounded(1);
    let thread = crate::XdcodeThread::new(
        session,
        SessionIo {
            tx_sub: submissions,
            rx_event: events,
            agent_status: watch::channel(AgentStatus::PendingInit).1,
            session_loop_termination: completed_session_loop_termination(),
        },
        crate::ThreadStartupMetadata::from(&configured),
        rollout_path,
        session_source,
    );
    (thread, receiver)
}

#[tokio::test]
async fn patch_approval_cancel_only_interrupts_its_requesting_turn() -> anyhow::Result<()> {
    const CALL_ID: &str = "patch";
    let (session, first, events) = make_session_and_context_with_rx().await;
    let (thread, submissions) = make_thread_for_session(
        Arc::clone(&session),
        &first,
        events.clone(),
        SessionSource::Exec,
    )
    .await;
    let dispatcher = tokio::spawn(submission_loop(
        Arc::clone(&session),
        Arc::clone(&first.config),
        submissions,
    ));
    let request_patch = |turn: Arc<TurnContext>| {
        let session = Arc::clone(&session);
        tokio::spawn(async move {
            session
                .spawn_task(
                    Arc::clone(&turn),
                    Vec::new(),
                    NeverEndingTask {
                        kind: TaskKind::Regular,
                        listen_to_cancellation_token: true,
                    },
                )
                .await;
            session
                .request_patch_approval(&turn, CALL_ID.into(), HashMap::new(), None, None)
                .await
        })
    };
    let cancel_first_at_next_prompt = async || {
        loop {
            if matches!(
                events.recv().await.expect("approval event").msg,
                EventMsg::ApplyPatchApprovalRequest(_)
            ) {
                break;
            }
        }
        thread
            .submit(Op::CancelPatchApproval {
                id: CALL_ID.into(),
                turn_id: first.sub_id.clone(),
            })
            .await
    };

    let first_waiter = request_patch(Arc::clone(&first));
    cancel_first_at_next_prompt().await?;
    assert_eq!(first_waiter.await?, ReviewDecision::Abort);
    let aborted = recv_terminal_event(&events, TerminalEventKind::TurnAborted).await;
    assert_eq!(aborted.id, first.sub_id);

    let second = session
        .new_turn_with_default_settings("next".into(), Default::default())
        .await;
    let second_waiter = request_patch(Arc::clone(&second));
    cancel_first_at_next_prompt().await?;
    thread
        .submit(Op::PatchApproval {
            id: CALL_ID.into(),
            decision: ReviewDecision::Approved,
        })
        .await?;
    assert_eq!(second_waiter.await?, ReviewDecision::Approved);
    assert!(
        session
            .turn_context_for_sub_id(&second.sub_id)
            .await
            .is_some()
    );
    drop(thread);
    dispatcher.await?;
    Ok(())
}
