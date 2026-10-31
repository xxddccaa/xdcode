//! Historical notice types; independent builds do not fetch hosted account reminders.

use crate::app_event_sender::AppEventSender;
use crate::app_server_session::AppServerSession;
use crate::legacy_core::config::Config;
use serde::Deserialize;
use uuid::Uuid;
use xdcode_login::XdcodeAuth;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Notice {
    pub title: String,
    pub description: String,
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Action {
    pub label: String,
    pub url: String,
}

#[derive(Deserialize)]
struct Response {
    notice: Option<Notice>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub account: String,
    pub user: String,
}
impl Identity {
    fn from_auth(auth: &XdcodeAuth) -> Option<Self> {
        Some(Self {
            account: auth.get_account_id()?,
            user: auth.get_chatgpt_user_id()?,
        })
    }
}
impl Notice {
    pub(crate) fn valid(&self) -> bool {
        !self.title.trim().is_empty()
            && self.title.len() <= 256
            && self.description.len() <= 2048
            && !self.title.chars().any(char::is_control)
            && !self.description.chars().any(char::is_control)
            && !self.action.label.trim().is_empty()
            && self.action.label.len() <= 128
            && !self.action.label.chars().any(char::is_control)
            && url::Url::parse(&self.action.url).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str() == Some("chatgpt.com")
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
            })
    }
}

pub(crate) fn prefetch(
    config: &Config,
    server: &AppServerSession,
    tx: AppEventSender,
    request_id: Uuid,
) {
    let _ = (config, server, tx, request_id);
}

#[cfg(test)]
#[path = "security_setup_tests.rs"]
mod tests;
