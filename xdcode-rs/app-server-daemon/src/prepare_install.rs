//! Start the daemon from the CLI package selected by a manual local installation.
//! Running daemons are preserved. No release discovery, download or updater exists.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use xdcode_install_context::InstallContext;
use xdcode_install_context::XdcodePackageManifest;

use crate::Daemon;
use crate::install_lock::acquire_install_lock;
use crate::managed_install;
use crate::settings::DaemonSettings;

/// Retained for callers of the unsupported package replacement API.
pub struct InstallRequest {
    pub source: PathBuf,
    pub version: String,
    pub destination: PathBuf,
    pub installed_version: Option<String>,
    pub restart_required: bool,
}

/// Prepare a missing package while the caller holds the daemon operation lock.
pub(super) async fn prepare(daemon: &Daemon, settings: &DaemonSettings) -> Result<()> {
    let source = InstallContext::current().package_layout.as_ref();
    Box::pin(prepare_from_package(
        daemon,
        settings,
        source.map(|layout| layout.package_dir.as_path()),
        &std::env::current_exe()?,
    ))
    .await
}

/// Updating an existing daemon package is unsupported, including from this CLI.
/// In particular, the confirmation callback is never invoked.
pub async fn update_from_cli(
    _confirm: impl FnOnce(&InstallRequest) -> Result<bool>,
) -> Result<Option<crate::UpdateOutput>> {
    anyhow::bail!(crate::UPDATES_UNSUPPORTED)
}

async fn prepare_from_package(
    daemon: &Daemon,
    settings: &DaemonSettings,
    source: Option<&Path>,
    running_exe: &Path,
) -> Result<()> {
    let home = daemon
        .settings_file
        .parent()
        .and_then(Path::parent)
        .context("daemon settings path has no Xdcode home")?;
    let previous_root = managed_install::package_root(home);
    let root = home.join("packages/app-server-daemon");
    anyhow::ensure!(
        daemon.managed_xdcode_bin.starts_with(&previous_root),
        "daemon package location changed; retry the command"
    );
    if previous_root != root
        || daemon.running_backend_instance(settings).await?.is_some()
        || crate::client::probe(&daemon.socket_path).await.is_ok()
    {
        return Ok(());
    }
    let current = root.join("current");
    let selection_before = selection_target(&current)?;
    if selection_before.is_some() {
        daemon.ensure_managed_xdcode_bin()?;
        if source.is_none() {
            return Ok(());
        }
    }
    let source = source.context(
        "this CLI has no complete local package; install a packaged xdcode CLI manually",
    )?;
    std::fs::create_dir_all(&root)?;
    anyhow::ensure!(
        !root.canonicalize()?.starts_with(source.canonicalize()?),
        "XDCODE_HOME must be outside the source CLI package"
    );
    let manifest_bytes = std::fs::read(source.join("xdcode-package.json"))?;
    let manifest: XdcodePackageManifest = serde_json::from_slice(&manifest_bytes)?;
    let version = manifest.version.to_string();
    let target = platform_target()?;
    let metadata: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    let entrypoint = if cfg!(windows) {
        "bin/xdcode.exe"
    } else {
        "bin/xdcode"
    };
    anyhow::ensure!(
        metadata["target"] == target && metadata["entrypoint"] == entrypoint,
        "the CLI package does not match this platform or executable"
    );
    validate_package(source)?;
    let source_digest = package_tree(source, /*destination*/ None)?;
    let releases = root.join("releases");
    let release = releases.join(format!("local-{source_digest}-{target}"));
    if current.canonicalize().ok().as_ref() == Some(&release) {
        return Ok(());
    }
    let running_identity = managed_install::executable_identity(running_exe).await?;
    daemon.diagnostic(format_args!(
        "Installing daemon from CLI version {version} into {}...",
        root.display()
    ));
    let _install_lock = acquire_install_lock(&root).await?;
    anyhow::ensure!(
        managed_install::package_root(home) == previous_root,
        "daemon package location changed; retry the command"
    );
    anyhow::ensure!(
        selection_target(&current)? == selection_before
            && daemon.running_backend_instance(settings).await?.is_none()
            && crate::client::probe(&daemon.socket_path).await.is_err(),
        "daemon selection or running state changed while preparing its package; retry the command"
    );
    std::fs::create_dir_all(&releases)?;
    let stage = tempfile::Builder::new()
        .prefix(".staging.")
        .tempdir_in(&releases)?;
    let digest = package_tree(source, Some(stage.path()))?;
    validate_package(stage.path())?;
    let staged_exe = if cfg!(target_os = "macos") && stage.path().join("XdcodeCLI.app").is_dir() {
        stage.path().join("XdcodeCLI.app/Contents/MacOS/xdcode")
    } else {
        stage.path().join(entrypoint)
    };
    anyhow::ensure!(
        digest == source_digest
            && package_tree(source, /*destination*/ None)? == digest
            && std::fs::read(stage.path().join("xdcode-package.json"))? == manifest_bytes
            && managed_install::executable_identity(&staged_exe)
                .await?
                .same_contents(&running_identity),
        "the CLI package changed while preparing the daemon or differs from the running executable"
    );
    let binary_version =
        managed_install::managed_xdcode_version(&stage.path().join(entrypoint)).await?;
    anyhow::ensure!(
        stable_version(&version).is_none() || version == binary_version,
        "the CLI package version does not match its executable"
    );
    if release.try_exists()? {
        anyhow::ensure!(
            !release.symlink_metadata()?.file_type().is_symlink()
                && package_tree(&release, /*destination*/ None)? == digest,
            "an existing daemon release has different contents; refusing to overwrite it"
        );
    } else {
        #[cfg(unix)]
        if !stage.path().join("xdcode").exists() {
            std::os::unix::fs::symlink("bin/xdcode", stage.path().join("xdcode"))?;
        }
        #[cfg(windows)]
        windows::publish_release(stage.path(), &release).await?;
        #[cfg(not(windows))]
        std::fs::rename(stage.path(), &release)?;
    }
    anyhow::ensure!(
        managed_install::package_root(home) == previous_root
            && selection_target(&current)? == selection_before
            && daemon.running_backend_instance(settings).await?.is_none()
            && crate::client::probe(&daemon.socket_path).await.is_err(),
        "daemon package location or socket ownership changed while preparing its package; retry the command"
    );
    #[cfg(windows)]
    windows::validate_selection(&root)?;
    #[cfg(unix)]
    {
        let temporary = tempfile::TempDir::new_in(&root)?;
        let link = temporary.path().join("current");
        std::os::unix::fs::symlink(&release, &link)?;
        std::fs::rename(link, root.join("current"))?;
    }
    #[cfg(windows)]
    windows::select_release(&root, &release)?;
    Ok(())
}

fn selection_target(current: &Path) -> Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(current) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_symlink(),
                "daemon current selection must be a symbolic link"
            );
            Ok(Some(std::fs::read_link(current)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Hash the complete tree and optionally copy those same bytes. Relative file
/// links are materialized; escaping links and directory links are rejected.
fn package_tree(root: &Path, destination: Option<&Path>) -> Result<String> {
    let canonical_root = root.canonicalize()?;
    let mut hasher = blake3::Hasher::new();
    let mut paths = vec![root.to_path_buf()];
    while let Some(path) = paths.pop() {
        let relative = path.strip_prefix(root)?;
        // The Unix installer adds this alias outside the package layout.
        if cfg!(unix)
            && relative == Path::new("xdcode")
            && std::fs::read_link(&path).ok().as_deref() == Some(Path::new("bin/xdcode"))
        {
            continue;
        }
        anyhow::ensure!(
            path.canonicalize()?.starts_with(&canonical_root),
            "package link escapes its root"
        );
        let metadata = path.metadata()?;
        hasher.update(relative.as_os_str().as_encoded_bytes());
        hasher.update(&[0]);
        if metadata.is_dir() {
            anyhow::ensure!(
                !path.symlink_metadata()?.file_type().is_symlink(),
                "package contains a directory link"
            );
            hasher.update(b"directory");
            if let Some(destination) = destination
                && !relative.as_os_str().is_empty()
            {
                std::fs::create_dir(destination.join(relative))?;
            }
            let mut entries = std::fs::read_dir(&path)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            entries.sort();
            paths.extend(entries);
        } else {
            anyhow::ensure!(metadata.is_file(), "package contains an unsupported file");
            let bytes = std::fs::read(&path)?;
            hasher.update(b"file");
            hasher.update(blake3::hash(&bytes).as_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                hasher.update(&(metadata.permissions().mode() & 0o777).to_le_bytes());
            }
            if let Some(destination) = destination {
                let target = destination.join(relative);
                std::fs::write(&target, bytes)?;
                std::fs::set_permissions(target, metadata.permissions())?;
            }
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn stable_version(value: &str) -> Option<semver::Version> {
    let version = semver::Version::parse(value).ok()?;
    (version.pre.is_empty()
        && version.build.is_empty()
        && (version.major, version.minor, version.patch) != (0, 0, 0))
        .then_some(version)
}

fn validate_package(root: &Path) -> Result<()> {
    let mut names = vec![
        "xdcode-package.json",
        if cfg!(windows) {
            "bin/xdcode.exe"
        } else {
            "bin/xdcode"
        },
        if cfg!(windows) {
            "bin/xdcode-code-mode-host.exe"
        } else {
            "bin/xdcode-code-mode-host"
        },
        if cfg!(windows) {
            "xdcode-path/rg.exe"
        } else {
            "xdcode-path/rg"
        },
    ];
    if cfg!(windows) {
        names.extend([
            "xdcode-resources/xdcode-command-runner.exe",
            "xdcode-resources/xdcode-windows-sandbox-setup.exe",
        ]);
    } else if cfg!(target_os = "linux") {
        names.push("xdcode-resources/bwrap");
    }
    for name in names {
        if !root.join(name).is_file() {
            return Err(anyhow!(
                "local Xdcode package is missing {name}; reinstall a complete local xdcode package"
            ));
        }
        #[cfg(unix)]
        if name != "xdcode-package.json" {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(root.join(name))?.permissions().mode() & 0o111 == 0 {
                return Err(anyhow!(
                    "local Xdcode package file {name} is not executable; reinstall a complete local xdcode package"
                ));
            }
        }
    }
    Ok(())
}

fn platform_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") if cfg!(target_env = "gnu") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") if cfg!(target_env = "gnu") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => Err(anyhow!("unsupported packaged daemon platform {os}/{arch}")),
    }
}

#[cfg(windows)]
#[path = "prepare_install_windows.rs"]
mod windows;

#[cfg(test)]
#[path = "prepare_install_tests.rs"]
mod tests;
