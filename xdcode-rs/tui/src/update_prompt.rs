//! The compatibility entry point never draws or offers an update prompt.

use crate::legacy_core::config::Config;
use crate::tui::Tui;
use crate::update_action::UpdateAction;

pub(crate) enum UpdatePromptOutcome {
    Continue,
    RunUpdate(UpdateAction),
}

pub(crate) async fn run_update_prompt_if_needed(
    _tui: &mut Tui,
    _config: &Config,
) -> color_eyre::Result<UpdatePromptOutcome> {
    Ok(UpdatePromptOutcome::Continue)
}
