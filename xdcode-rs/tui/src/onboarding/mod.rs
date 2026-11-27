mod keys;
pub(crate) mod onboarding_screen;
mod provider_setup;
pub(crate) use provider_setup::FieldDisplay;
pub(crate) use provider_setup::ProviderInput;
pub(crate) use provider_setup::move_provider_field;
mod trust_directory;
pub(crate) use crate::terminal_hyperlinks::mark_underlined_hyperlink;
pub(crate) use crate::terminal_hyperlinks::mark_url_hyperlink;
pub(crate) use onboarding_screen::DirectoryTrustOptions;
pub(crate) use trust_directory::TrustCancelAction;
mod welcome;

pub(crate) const SETUP_CONNECTION_LOST: &str =
    "Xdcode lost its connection during setup. Restart Xdcode to continue.";
