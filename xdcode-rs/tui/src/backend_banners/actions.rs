//! Map backend CTA names to existing CLI actions and desktop-equivalent browser destinations.

use super::BackendBanner;
use xdcode_app_server_protocol::AddCreditsNudgeCreditType;
use xdcode_protocol::account::PlanType;

pub(super) enum BannerAction {
    OpenUrl(String),
    NotifyOwner(AddCreditsNudgeCreditType),
    ResetUsage,
}

impl BackendBanner {
    pub(super) fn resolve_action(&self, _action: &str) -> Option<BannerAction> {
        None
    }
}
