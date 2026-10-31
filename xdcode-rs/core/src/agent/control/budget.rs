//! Keeps shared rollout-budget accounting and reminder state behind the controller.
//! Callers acknowledge a reminder only after it has been added to conversation history.

use super::LocalAgentControl;
use crate::rollout_budget::RolloutBudgetReminder;
use xdcode_protocol::ThreadId;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_protocol::protocol::TokenUsage;

impl LocalAgentControl {
    pub(crate) fn record_rollout_budget_usage(&self, usage: &TokenUsage) -> XdcodeResult<()> {
        if self.runtime.rollout_budget.record_usage(usage)? {
            return Err(XdcodeErr::SessionBudgetExceeded);
        }
        Ok(())
    }

    pub(crate) fn pending_budget_reminder(
        &self,
        thread_id: ThreadId,
        window_id: &str,
    ) -> Option<RolloutBudgetReminder> {
        self.runtime
            .rollout_budget
            .pending_reminder(thread_id, window_id)
    }

    pub(crate) fn mark_budget_reminder_delivered(
        &self,
        thread_id: ThreadId,
        window_id: &str,
        reminder: RolloutBudgetReminder,
    ) {
        self.runtime
            .rollout_budget
            .mark_reminder_delivered(thread_id, window_id, reminder);
    }
}
