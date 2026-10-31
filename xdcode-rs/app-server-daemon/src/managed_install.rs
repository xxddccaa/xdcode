//! Resolves both package and legacy standalone layouts and compares installed executables.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde::Deserialize;
use serde::Serialize;
use tokio::process::Command;

/// New daemons own their packages, regardless of how the calling CLI was installed.
/// Preserve legacy launch state, including logs left after a daemon is stopped;
/// settings, installer selections, and lock files alone do not prove a prior launch.
pub(crate) fn package_root(xdcode_home: &Path) -> PathBuf {
    let dedicated = xdcode_home.join("packages/app-server-daemon");
    if !matches!(dedicated.join("current").symlink_metadata(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return dedicated;
    }
    let state = xdcode_home.join("app-server-daemon");
    for (package, artifacts) in [
        (
            "app-server-daemon",
            [
                crate::DAEMON_PID_FILE_NAME,
                "daemon.stderr.log",
                crate::DAEMON_UPDATE_PID_FILE_NAME,
                "daemon-updater.stderr.log",
            ],
        ),
        (
            "standalone",
            [
                crate::LEGACY_PID_FILE_NAME,
                "app-server.stderr.log",
                crate::LEGACY_UPDATE_PID_FILE_NAME,
                "app-server-updater.stderr.log",
            ],
        ),
    ] {
        if artifacts.iter().any(|name| {
            !matches!(state.join(name).symlink_metadata(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        }) {
            return xdcode_home.join("packages").join(package);
        }
    }
    dedicated
}

/// Resolve both packaged and legacy binaries without requiring a valid install.
pub(crate) fn managed_xdcode_bin(xdcode_home: &Path) -> PathBuf {
    let root = package_root(xdcode_home);
    let current = root.join("current");
    let packaged = current.join("bin").join(managed_xdcode_file_name());
    let legacy = current.join(managed_xdcode_file_name());
    if packaged.is_file()
        || !legacy.is_file() && (cfg!(windows) || root.ends_with("app-server-daemon"))
    {
        packaged
    } else {
        legacy
    }
}

pub(crate) async fn managed_xdcode_version(xdcode_bin: &Path) -> Result<String> {
    let mut command = Command::new(xdcode_bin);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let output = command
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| {
            format!(
                "failed to invoke managed Xdcode binary {}",
                xdcode_bin.display()
            )
        })?;
    if !output.status.success() {
        return Err(anyhow!(
            "managed Xdcode binary {} exited with status {}",
            xdcode_bin.display(),
            output.status
        ));
    }

    let stdout = String::from_utf8(output.stdout).with_context(|| {
        format!(
            "managed Xdcode version was not utf-8: {}",
            xdcode_bin.display()
        )
    })?;
    parse_xdcode_version(&stdout)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExecutableIdentity {
    pub(crate) digest: [u8; 32],
    // Distinguish release generations whose executable bytes are identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) path_digest: Option<[u8; 32]>,
}

impl ExecutableIdentity {
    pub(crate) fn same_contents(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}

pub(crate) async fn executable_identity(executable: &Path) -> Result<ExecutableIdentity> {
    let executable = executable.to_path_buf();
    // Debug executables can be hundreds of MB. Stream the digest off the async
    // runtime instead of allocating the whole file and blocking a runtime thread.
    tokio::task::spawn_blocking(move || {
        let executable = std::fs::canonicalize(&executable).unwrap_or(executable);
        let mut identity = std::fs::File::open(&executable)
            .and_then(executable_identity_from_reader)
            .with_context(|| format!("failed to read executable {}", executable.display()))?;
        identity.path_digest = Some(path_digest(&executable));
        Ok(identity)
    })
    .await
    .context("executable identity task failed")?
}

pub(crate) fn executable_identity_from_reader(
    reader: impl std::io::Read,
) -> std::io::Result<ExecutableIdentity> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(reader)?;
    Ok(ExecutableIdentity {
        digest: *hasher.finalize().as_bytes(),
        path_digest: None,
    })
}

pub(crate) fn path_digest(path: &Path) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            hasher.update(&unit.to_le_bytes());
        }
    }
    *hasher.finalize().as_bytes()
}

fn managed_xdcode_file_name() -> &'static str {
    if cfg!(windows) {
        "xdcode.exe"
    } else {
        "xdcode"
    }
}

fn parse_xdcode_version(output: &str) -> Result<String> {
    let version = output
        .split_whitespace()
        .nth(1)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| anyhow!("managed Xdcode version output was malformed"))?;
    Ok(version.to_string())
}

#[cfg(test)]
#[path = "managed_install_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "managed_install_path_tests.rs"]
mod path_tests;
