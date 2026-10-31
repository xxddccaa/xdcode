use super::*;
use crate::app::test_support::make_test_app;
use crate::chatwidget::tests::helpers::render_bottom_popup;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::status::remote_connection::RemoteConnectionStatus;
use crossterm::event::KeyCode;

#[tokio::test]
async fn managed_daemon_menu_requires_confirmation_and_offers_only_restart() {
    let mut app = make_test_app().await;
    let (chat, _, mut rx, _) = make_chatwidget_manual_with_sender().await;
    app.chat_widget = chat;
    app.app_server_target = AppServerTarget::LocalDaemon {
        allow_embedded_fallback: true,
        endpoint: crate::RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::relative_to_current_dir("xdcode.sock").unwrap(),
        },
    };
    app.chat_widget.remote_connection = Some(RemoteConnectionStatus {
        address: "local".into(),
        version: "v0.163.0".into(),
        is_local_daemon: true,
        is_managed_daemon: true,
    });

    app.open_daemon_menu();
    let menu = render_bottom_popup(&app.chat_widget, 100);
    assert!(menu.contains("Restart background server"));
    assert!(!menu.contains("Install"));
    assert!(!menu.contains("Use this CLI"));
    assert!(rx.try_recv().is_err());
    app.chat_widget.handle_key_event(KeyCode::Enter.into());
    assert!(matches!(
        rx.try_recv().unwrap(),
        AppEvent::ConfirmDaemonRestart
    ));

    app.confirm_daemon_restart();
    let confirmation = render_bottom_popup(&app.chat_widget, 100);
    assert!(confirmation.contains("Active or queued work"));
    assert!(confirmation.contains("may be interrupted"));
    // Cancel is the default, so opening and accepting this view cannot restart anything.
    app.chat_widget.handle_key_event(KeyCode::Enter.into());
    assert!(rx.try_recv().is_err());
    app.confirm_daemon_restart();
    app.chat_widget.handle_key_event(KeyCode::Down.into());
    app.chat_widget.handle_key_event(KeyCode::Enter.into());
    assert!(matches!(rx.try_recv().unwrap(), AppEvent::RunDaemonRestart));
    assert!(app.daemon_restart.is_none());
}

#[tokio::test]
async fn unverified_remote_and_offline_connections_cannot_request_restart() {
    let mut app = make_test_app().await;
    let (chat, _, mut rx, _) = make_chatwidget_manual_with_sender().await;
    app.chat_widget = chat;
    app.app_server_target = AppServerTarget::LocalDaemon {
        allow_embedded_fallback: true,
        endpoint: crate::RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::relative_to_current_dir("xdcode.sock").unwrap(),
        },
    };
    for (is_local_daemon, is_managed_daemon, offline) in [
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        app.chat_widget.remote_connection = Some(RemoteConnectionStatus {
            address: "test".into(),
            version: "v0.163.0".into(),
            is_local_daemon,
            is_managed_daemon,
        });
        app.reconnect.offline = offline;
        app.open_daemon_menu();
        let menu = render_bottom_popup(&app.chat_widget, 100);
        assert!(menu.contains("disabled"));
        app.confirm_daemon_restart();
        assert_eq!(render_bottom_popup(&app.chat_widget, 100), menu);
        app.chat_widget.handle_key_event(KeyCode::Enter.into());
        assert!(rx.try_recv().is_err());
        assert!(app.daemon_restart.is_none());
    }
}
