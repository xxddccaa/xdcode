//! A mode-bound message is sent only after the server accepts its mode and provider settings.

use super::*;
use xdcode_app_server_protocol::ThreadSettingsUpdateParams;
use xdcode_protocol::config_types::CollaborationModeMask;
use xdcode_protocol::config_types::ModeKind;

impl App {
    pub(super) async fn submit_mode_message(
        &mut self,
        app_server: &mut AppServerSession,
        text: String,
        mask: CollaborationModeMask,
    ) {
        if !self.chat_widget.can_submit_mode_message() {
            self.chat_widget.restore_mode_message(text);
            return;
        }
        let Some(thread_id) = self.active_thread_id else {
            self.chat_widget.restore_mode_message(text);
            return;
        };
        let kind = mask.mode.unwrap_or(ModeKind::Default);
        let updated = match self.chat_widget.catalog_mode_selection(kind) {
            Ok(Some(selection)) => self.switch_catalog_mode(app_server, selection).await,
            Ok(None) => {
                let mask = self.chat_widget.legacy_mode_mask(mask);
                let mut mode = self
                    .chat_widget
                    .effective_collaboration_mode()
                    .apply_mask(&mask);
                mode.mode = kind;
                mode.settings.developer_instructions = None;
                let updated = self
                    .send_thread_settings_update(
                        app_server,
                        ThreadSettingsUpdateParams {
                            thread_id: thread_id.to_string(),
                            collaboration_mode: Some(mode.clone()),
                            ..ThreadSettingsUpdateParams::default()
                        },
                    )
                    .await;
                if updated {
                    self.chat_widget.set_effective_collaboration_mode(mode);
                }
                updated
            }
            Err(error) => {
                self.chat_widget.add_error_message(error);
                false
            }
        };
        if updated && self.active_thread_id == Some(thread_id) {
            self.chat_widget.submit_confirmed_mode_message(text);
        } else {
            self.chat_widget.add_warning_message(
                "The message was not sent because the mode change was not accepted.".into(),
            );
            self.chat_widget.restore_mode_message(text);
        }
    }
}
