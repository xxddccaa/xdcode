//! Exercise mode-bound submission against the public app-server settings RPC.
use super::*;
use pretty_assertions::assert_eq;
use xdcode_app_server_client::AppServerClient;
use xdcode_app_server_client::CloudClient;
use xdcode_app_server_client::CloudOperation;
use xdcode_protocol::config_types::Settings;

#[tokio::test]
async fn rejected_mode_change_preserves_plan_and_restores_message() -> Result<()> {
    let (mut app, _events, mut ops) = make_test_app_with_channels().await;
    let thread_id = ThreadId::new();
    app.active_thread_id = Some(thread_id);
    app.chat_widget.handle_thread_session(test_thread_session(
        thread_id,
        test_path_buf("/tmp/project"),
    ));
    app.chat_widget
        .set_effective_collaboration_mode(CollaborationMode {
            mode: ModeKind::Plan,
            settings: Settings {
                model: "test-model".into(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        });
    while let Ok(operation) = ops.try_recv() {
        assert!(!matches!(operation, Op::UserTurn { .. }));
    }
    let mask = crate::collaboration_modes::default_mode_mask(&app.model_catalog).unwrap();
    let (client, mut commands, _notifications) = CloudClient::channel(1);
    let mut server = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        AppServerClient::Cloud(client),
        crate::app_server_session::ThreadParamsMode::Embedded,
    );
    {
        let request = app.submit_mode_message(&mut server, "Implement the plan.".into(), mask);
        tokio::pin!(request);
        assert!(futures::poll!(&mut request).is_pending());
        let command = commands.recv().await.expect("settings request");
        assert!(matches!(command.operation, CloudOperation::Request(ref r)
        if matches!(r.as_ref(), xdcode_app_server_protocol::ClientRequest::ThreadSettingsUpdate { params, .. }
            if params.thread_id == thread_id.to_string() && params.collaboration_mode.as_ref().unwrap().mode == ModeKind::Default)));
        assert!(
            ops.try_recv().is_err(),
            "must not submit a turn before acknowledgment"
        );
        command
            .response
            .send(Err(std::io::Error::other("mode rejected")))
            .unwrap();
        request.await;
    }
    assert_eq!(
        app.chat_widget.effective_collaboration_mode().mode,
        ModeKind::Plan
    );
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "Implement the plan."
    );
    assert!(ops.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn accepted_plan_to_default_change_submits_the_message_in_default_mode() -> Result<()> {
    let (mut app, _events, mut ops) = make_test_app_with_channels().await;
    let thread_id = ThreadId::new();
    app.active_thread_id = Some(thread_id);
    app.chat_widget.handle_thread_session(test_thread_session(
        thread_id,
        test_path_buf("/tmp/project"),
    ));
    app.chat_widget
        .set_effective_collaboration_mode(CollaborationMode {
            mode: ModeKind::Plan,
            settings: Settings {
                model: "test-model".into(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        });
    while let Ok(operation) = ops.try_recv() {
        assert!(!matches!(operation, Op::UserTurn { .. }));
    }
    let mask = crate::collaboration_modes::default_mode_mask(&app.model_catalog).unwrap();
    let (client, mut commands, _notifications) = CloudClient::channel(1);
    let mut server = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        AppServerClient::Cloud(client),
        crate::app_server_session::ThreadParamsMode::Embedded,
    );

    {
        let request = app.submit_mode_message(&mut server, "Implement the plan.".into(), mask);
        tokio::pin!(request);
        assert!(futures::poll!(&mut request).is_pending());
        let command = commands.recv().await.expect("settings request");
        assert!(
            matches!(command.operation, CloudOperation::Request(ref request)
            if matches!(request.as_ref(), xdcode_app_server_protocol::ClientRequest::ThreadSettingsUpdate { params, .. }
                if params.thread_id == thread_id.to_string() && params.collaboration_mode.as_ref().unwrap().mode == ModeKind::Default))
        );
        assert!(
            ops.try_recv().is_err(),
            "must not submit a turn before acknowledgment"
        );
        command
            .response
            .send(Ok(Ok(serde_json::to_value(
                xdcode_app_server_protocol::ThreadSettingsUpdateResponse {},
            )?)))
            .unwrap();
        request.await;
    }

    assert_eq!(
        app.chat_widget.effective_collaboration_mode().mode,
        ModeKind::Default
    );
    let Op::UserTurn {
        items,
        collaboration_mode,
        ..
    } = ops
        .try_recv()
        .expect("accepted message should start a turn")
    else {
        panic!("expected a user turn");
    };
    assert_eq!(
        items,
        vec![AppServerUserInput::Text {
            text: "Implement the plan.".into(),
            text_elements: Vec::new(),
        }]
    );
    assert_eq!(
        collaboration_mode
            .expect("turn should carry its accepted mode")
            .mode,
        ModeKind::Default
    );
    assert!(ops.try_recv().is_err());
    Ok(())
}
