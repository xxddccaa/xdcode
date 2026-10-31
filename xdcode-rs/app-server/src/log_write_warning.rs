use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use xdcode_app_server_protocol::ServerNotification;
use xdcode_app_server_protocol::WarningNotification;
use xdcode_core::config::Config;
use xdcode_feedback::XdcodeFeedback;
use xdcode_state::LogWriteFailureReporter;

use crate::outgoing_message::OutgoingMessageSender;

const LOG_WRITE_WARNING_WITH_FEEDBACK: &str = "Xdcode couldn't save diagnostic logs to its local database. Use /feedback with logs included before closing Xdcode, or run `xdcode doctor` for diagnostics.";
const LOG_WRITE_WARNING_WITHOUT_FEEDBACK: &str = "Xdcode couldn't save diagnostic logs to its local database. Run `xdcode doctor` for diagnostics.";

pub(crate) struct LogWriteWarningReporter {
    feedback: XdcodeFeedback,
    outgoing: Weak<OutgoingMessageSender>,
    /// Permanently set by the first failure; limits warning delivery to one attempt
    /// for this reporter, even if later SQLite writes succeed.
    has_failure: AtomicBool,
    message: &'static str,
}

impl LogWriteWarningReporter {
    pub(crate) fn new(
        feedback: XdcodeFeedback,
        outgoing: &Arc<OutgoingMessageSender>,
        config: &Config,
    ) -> Arc<Self> {
        Arc::new(Self {
            feedback,
            outgoing: Arc::downgrade(outgoing),
            has_failure: AtomicBool::new(false),
            message: if config.feedback_enabled {
                LOG_WRITE_WARNING_WITH_FEEDBACK
            } else {
                LOG_WRITE_WARNING_WITHOUT_FEEDBACK
            },
        })
    }

    pub(crate) fn notify_failure(&self) {
        if !self.has_failure.swap(true, Ordering::Relaxed)
            && let Some(outgoing) = self.outgoing.upgrade()
        {
            outgoing.try_send_server_notification(ServerNotification::Warning(
                WarningNotification {
                    thread_id: None,
                    message: self.message.to_string(),
                },
            ));
        }
    }
}

impl LogWriteFailureReporter for LogWriteWarningReporter {
    fn report_failure(&self, diagnostic: &str) {
        self.feedback.report_failure(diagnostic);
        self.notify_failure();
    }
}

#[cfg(test)]
#[path = "log_write_warning_tests.rs"]
mod tests;
