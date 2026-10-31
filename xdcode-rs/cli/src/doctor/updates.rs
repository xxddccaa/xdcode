//! Independent builds do not probe for or download upstream updates.

use super::{CheckStatus, DoctorCheck};
use xdcode_core::config::Config;

pub(super) async fn updates_check(_config: &Config) -> DoctorCheck {
    DoctorCheck::new(
        "updates.status",
        "updates",
        CheckStatus::Ok,
        "automatic updates disabled",
    )
}
