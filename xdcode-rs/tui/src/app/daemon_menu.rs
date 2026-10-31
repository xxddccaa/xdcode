//! Inspect and restart the local xdcode daemon without downloading or replacing its package.

use super::*;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::wrapping::word_wrap_lines;
use ratatui::buffer::Buffer;
use ratatui::widgets::Paragraph;

struct DaemonMenuHeader(Vec<Line<'static>>);

impl Renderable for DaemonMenuHeader {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Renderable::render(
            &Paragraph::new(word_wrap_lines(&self.0, usize::from(area.width))),
            area,
            buf,
        );
    }

    fn desired_height(&self, width: u16) -> u16 {
        word_wrap_lines(&self.0, usize::from(width)).len() as u16
    }
}

impl App {
    pub(super) fn daemon_restart_available(&self) -> bool {
        matches!(self.app_server_target, AppServerTarget::LocalDaemon { .. })
            && !self.reconnect.offline
            && self.daemon_restart.is_none()
            && self
                .chat_widget
                .remote_connection
                .as_ref()
                .is_some_and(|connection| {
                    connection.is_local_daemon && connection.is_managed_daemon
                })
    }

    pub(super) async fn verify_managed_daemon(&mut self) {
        // A discovered socket alone does not prove it belongs to a managed daemon.
        // Probe only when the user opens a view that offers restart, never during startup.
        if !self.reconnect.offline
            && let Some(connection) = &mut self.chat_widget.remote_connection
            && connection.is_local_daemon
            && !connection.is_managed_daemon
        {
            connection.is_managed_daemon =
                xdcode_app_server_daemon::run(xdcode_app_server_daemon::LifecycleCommand::Version)
                    .await
                    .is_ok_and(|output| output.backend.is_some());
        }
    }

    pub(super) fn open_daemon_menu(&mut self) {
        let status = self
            .chat_widget
            .remote_connection
            .as_ref()
            .filter(|_| matches!(self.app_server_target, AppServerTarget::LocalDaemon { .. }))
            .map(|connection| format!("Running daemon: {}", connection.version))
            .unwrap_or_else(|| "Not connected to the local background server.".to_string());
        let header = vec![Line::from("Daemon".bold()), Line::from(status.dim())];
        let items = vec![SelectionItem {
            name: "Restart background server".to_string(),
            is_disabled: !self.daemon_restart_available(),
            disabled_reason: self
                .chat_widget
                .remote_connection
                .as_ref()
                .filter(|connection| connection.is_local_daemon && !connection.is_managed_daemon)
                .map(|_| {
                    "Xdcode hasn't verified it manages this server. Restart it where it was started."
                        .to_string()
                }),
            actions: vec![Box::new(|tx| tx.send(AppEvent::ConfirmDaemonRestart))],
            dismiss_on_select: true,
            ..Default::default()
        }];
        self.chat_widget.show_selection_view(SelectionViewParams {
            header: Box::new(DaemonMenuHeader(header)),
            items,
            ..SelectionViewParams::picker()
        });
    }

    pub(super) fn confirm_daemon_restart(&mut self) {
        if !self.daemon_restart_available() {
            return;
        }
        self.chat_widget.show_selection_view(SelectionViewParams {
            header: Box::new(DaemonMenuHeader(vec![
                Line::from("Restart background server?".bold()),
                Line::from("Active or queued work in other chats may be interrupted."),
                Line::from("Xdcode will reconnect automatically."),
            ])),
            items: vec![
                SelectionItem {
                    name: "Cancel".to_string(),
                    dismiss_on_select: true,
                    ..Default::default()
                },
                SelectionItem {
                    name: "Restart".to_string(),
                    actions: vec![Box::new(|tx| tx.send(AppEvent::RunDaemonRestart))],
                    require_explicit_confirmation: true,
                    dismiss_on_select: true,
                    ..Default::default()
                },
            ],
            ..SelectionViewParams::confirmation()
        });
    }
}

#[cfg(test)]
#[path = "daemon_menu_tests.rs"]
mod tests;
