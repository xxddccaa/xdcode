use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{self, Command};

fn valid_identity(identity: &str) -> bool {
    if identity.len() > 100 {
        return false;
    }
    let Some((version, build_id)) = identity.rsplit_once('-') else {
        return false;
    };
    let components: Vec<_> = version.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && (component.len() == 1 || !component.starts_with('0'))
        })
        && build_id.len() == 13
        && build_id.starts_with('g')
        && build_id
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(windows)]
fn preserve_console_wait() {
    unsafe extern "system" fn handle_signal(event: u32) -> i32 {
        i32::from(event == 0 || event == 1)
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    unsafe {
        SetConsoleCtrlHandler(Some(handle_signal), 1);
    }
}

fn run() -> io::Result<i32> {
    let executable = env::current_exe()?;
    let install_root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("cannot locate the xdcode installation root"))?;
    let selection = fs::read_to_string(install_root.join("bin").join("current.txt"))?;
    let identity = selection.trim();
    if !valid_identity(identity) {
        return Err(io::Error::other("invalid local xdcode build selection"));
    }
    let binary = install_root
        .join("builds")
        .join(identity)
        .join("bin")
        .join("xdcode.exe");
    #[cfg(windows)]
    preserve_console_wait();
    let status = Command::new(binary).args(env::args_os().skip(1)).status()?;
    Ok(status.code().unwrap_or(1))
}

fn main() {
    match run() {
        Ok(status) => process::exit(status),
        Err(error) => {
            eprintln!("xdcode launcher: {error}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::valid_identity;

    #[test]
    fn accepts_only_safe_release_identities() {
        assert!(valid_identity("0.167.0-gaaaaaaaaaaaa"));
        for identity in [
            "../outside",
            "0.167.0-gAAAAAAAAAAAA",
            "0.167.0-gaaaaaaaaaaaa/../outside",
            "0.167.0-gaaaaaaaaaaaa\\outside",
            "0.167.0-gaaaaaaaaaaaa:stream",
            "01.167.0-gaaaaaaaaaaaa",
            "0.167.0-g123",
        ] {
            assert!(!valid_identity(identity), "{identity}");
        }
    }
}
