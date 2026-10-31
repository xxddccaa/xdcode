//! Independent providers have no hosted account identity to refresh.

use super::App;
use crate::app_server_session::AppServerSession;

impl App {
    pub(super) fn refresh_account_email(&mut self, _app_server: &AppServerSession) {
        self.account_email_request_id = None;
    }
}
