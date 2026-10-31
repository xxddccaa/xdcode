//! Backend banner payload and CTA mapping, supplied by the existing account usage read.
//!
//! The backend owns eligibility, copy, actions, and ordered model fallback instructions.

use serde::Deserialize;
use xdcode_protocol::account::PlanType;

mod actions;
mod render;

pub(crate) const LUNA_RESERVE_BANNER: &str = "luna_reserve";
pub(crate) const LUNA_RESERVE_RECOVERY_VIEW_ID: &str = "luna-reserve-recovery";

#[cfg(test)]
#[path = "backend_banners_tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct BackendBanner {
    pub(crate) banner_type: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) ctas: Vec<BackendBannerCta>,
    pub(crate) reset_at: Option<i64>,
    pub(crate) model_slug: Option<String>,
    pub(crate) blocked_model_slug: Option<String>,
    #[serde(default)]
    pub(crate) fallback_model_slugs: Vec<String>,
    #[serde(default)]
    pub(crate) presentation: BannerPresentation,
    request_url: Option<String>,
    #[serde(skip)]
    pub(crate) account_id: String,
    #[serde(skip)]
    pub(crate) plan_type: Option<PlanType>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BannerPresentation {
    #[default]
    Inline,
    Dismissible,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct BackendBannerCta {
    action: String,
    label: String,
}

impl BackendBanner {
    /// Parse supported, bounded content before constructing rendered copy or CTA closures.
    pub(crate) fn parse(_raw: &serde_json::Value) -> Option<Self> {
        None
    }
}
