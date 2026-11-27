mod bel;
mod osc;

use std::io;

use bel::BelBackend;
use osc::OscBackend;
use osc::OscProtocol;
use xdcode_config::types::NotificationMethod;
use xdcode_terminal_detection::TerminalInfo;
use xdcode_terminal_detection::TerminalName;
use xdcode_terminal_detection::terminal_info;

#[derive(Debug)]
pub enum DesktopNotificationBackend {
    Osc(OscBackend),
    Bel(BelBackend),
}

impl DesktopNotificationBackend {
    pub fn for_method(method: NotificationMethod) -> Self {
        match method {
            NotificationMethod::Auto => {
                if supports_osc9(&terminal_info()) {
                    Self::Osc(OscBackend::new(OscProtocol::Osc9))
                } else {
                    Self::Bel(BelBackend)
                }
            }
            NotificationMethod::Osc9 => Self::Osc(OscBackend::new(OscProtocol::Osc9)),
            NotificationMethod::Osc777 => Self::Osc(OscBackend::new(OscProtocol::Osc777)),
            NotificationMethod::Bel => Self::Bel(BelBackend),
        }
    }

    pub fn method(&self) -> NotificationMethod {
        match self {
            DesktopNotificationBackend::Osc(backend) => match backend.protocol {
                OscProtocol::Osc9 => NotificationMethod::Osc9,
                OscProtocol::Osc777 => NotificationMethod::Osc777,
            },
            DesktopNotificationBackend::Bel(_) => NotificationMethod::Bel,
        }
    }

    pub fn notify(&mut self, message: &str) -> io::Result<()> {
        match self {
            DesktopNotificationBackend::Osc(backend) => backend.notify(message),
            DesktopNotificationBackend::Bel(backend) => backend.notify(message),
        }
    }
}

pub fn detect_backend(method: NotificationMethod) -> DesktopNotificationBackend {
    DesktopNotificationBackend::for_method(method)
}

fn supports_osc9(terminal: &TerminalInfo) -> bool {
    matches!(
        terminal.name,
        TerminalName::Ghostty
            | TerminalName::Iterm2
            | TerminalName::Kitty
            | TerminalName::WarpTerminal
            | TerminalName::WezTerm
    )
}

#[cfg(test)]
mod tests {
    use super::detect_backend;
    use super::supports_osc9;
    use pretty_assertions::assert_eq;
    use xdcode_config::types::NotificationMethod;
    use xdcode_terminal_detection::TerminalInfo;
    use xdcode_terminal_detection::TerminalName;

    fn test_terminal(name: TerminalName) -> TerminalInfo {
        TerminalInfo {
            backend: None,
            name,
            term_program: None,
            version: None,
            term: None,
            multiplexer: None,
        }
    }

    #[test]
    fn selects_explicit_notification_method() {
        for method in [
            NotificationMethod::Osc9,
            NotificationMethod::Osc777,
            NotificationMethod::Bel,
        ] {
            assert_eq!(detect_backend(method).method(), method);
        }
    }

    #[test]
    fn supports_osc9_for_supported_terminals() {
        for name in [
            TerminalName::Ghostty,
            TerminalName::Iterm2,
            TerminalName::Kitty,
            TerminalName::WarpTerminal,
            TerminalName::WezTerm,
        ] {
            assert!(
                supports_osc9(&test_terminal(name)),
                "{name:?} should support OSC 9"
            );
        }
    }

    #[test]
    fn supports_osc9_for_unsupported_terminals() {
        for name in [
            TerminalName::AppleTerminal,
            TerminalName::Alacritty,
            TerminalName::Dumb,
            TerminalName::GnomeTerminal,
            TerminalName::Konsole,
            TerminalName::Unknown,
            TerminalName::VsCode,
            TerminalName::Vte,
            TerminalName::WindowsTerminal,
        ] {
            assert_eq!(
                supports_osc9(&test_terminal(name)),
                false,
                "{name:?} should not support OSC 9"
            );
        }
    }
}
