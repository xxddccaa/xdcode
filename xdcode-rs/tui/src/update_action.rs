//! Compatibility types for callers carrying an update action in an exit result.
//! No action can discover or execute an updater in xdcode.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    Daemon(DaemonUpdateSource),
    NpmGlobalLatest,
    BunGlobalLatest,
    VitePlusGlobalLatest,
    PnpmGlobalLatest,
    BrewUpgrade,
    StandaloneUnix,
    StandaloneWindows,
}

const UNSUPPORTED_MESSAGE: &str =
    "Automatic updates are unsupported in xdcode; install a local xdcode package manually.";

impl UpdateAction {
    pub fn command_args(self) -> std::io::Result<(&'static str, &'static [&'static str])> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            UNSUPPORTED_MESSAGE,
        ))
    }

    pub fn command_str(self) -> String {
        UNSUPPORTED_MESSAGE.to_string()
    }
}

pub fn get_update_action() -> Option<UpdateAction> {
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonUpdateSource {
    PublicStable,
    ThisCli,
}

impl DaemonUpdateSource {
    pub fn command_args(self) -> std::io::Result<&'static [&'static str]> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            UNSUPPORTED_MESSAGE,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_install_method_can_supply_an_update_command() {
        assert_eq!(get_update_action(), None);
        for action in [
            UpdateAction::Daemon(DaemonUpdateSource::PublicStable),
            UpdateAction::Daemon(DaemonUpdateSource::ThisCli),
            UpdateAction::NpmGlobalLatest,
            UpdateAction::BunGlobalLatest,
            UpdateAction::VitePlusGlobalLatest,
            UpdateAction::PnpmGlobalLatest,
            UpdateAction::BrewUpgrade,
            UpdateAction::StandaloneUnix,
            UpdateAction::StandaloneWindows,
        ] {
            let error = action.command_args().expect_err("updating is unsupported");
            assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
            assert!(action.command_str().contains("unsupported in xdcode"));
        }
        for source in [
            DaemonUpdateSource::PublicStable,
            DaemonUpdateSource::ThisCli,
        ] {
            assert_eq!(
                source
                    .command_args()
                    .expect_err("updating is unsupported")
                    .kind(),
                std::io::ErrorKind::Unsupported
            );
        }
    }
}
