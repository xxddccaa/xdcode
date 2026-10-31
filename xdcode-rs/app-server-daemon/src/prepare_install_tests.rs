//! Covers complete-package staging, conservative selection, and legacy preservation.
#![cfg(unix)]
use super::prepare_from_package;
use super::validate_package;
use crate::settings::DaemonSettings;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;

fn daemon(home: &std::path::Path) -> crate::Daemon {
    let state = home.join("app-server-daemon");
    crate::Daemon {
        log_diagnostics: false,
        socket_path: state.join("app-server.sock"),
        pid_file: state.join("app-server.pid"),
        update_pid_file: state.join("app-server-updater.pid"),
        operation_lock_file: state.join("daemon.lock"),
        settings_file: state.join("settings.json"),
        managed_xdcode_bin: crate::managed_install::managed_xdcode_bin(home),
    }
}

fn package(root: &Path, version: &str) -> PathBuf {
    let target = super::platform_target().expect("target");
    for dir in ["bin", "xdcode-path", "xdcode-resources/nested"] {
        std::fs::create_dir_all(root.join(dir)).expect("package directory");
    }
    let bin = root.join("bin/xdcode");
    xdcode_utils_cargo_bin::write_executable(
        &bin,
        &format!("#!/bin/sh\necho 'xdcode {version}'\n"),
    )
    .expect("xdcode executable");
    for file in ["bin/xdcode-code-mode-host", "xdcode-path/rg"] {
        xdcode_utils_cargo_bin::write_executable(&root.join(file), "runtime")
            .expect("executable helper");
    }
    std::fs::write(root.join("xdcode-resources/nested/runtime"), b"runtime").expect("package file");
    if cfg!(target_os = "linux") {
        xdcode_utils_cargo_bin::write_executable(&root.join("xdcode-resources/bwrap"), "runtime")
            .expect("executable bwrap");
    }
    std::fs::write(
        root.join("xdcode-package.json"),
        serde_json::json!({
            "version": version, "target": target, "entrypoint": "bin/xdcode"
        })
        .to_string(),
    )
    .expect("manifest");
    bin
}

#[tokio::test]
async fn seeds_full_package() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().join("home");
    let daemon = daemon(&home);
    let settings = DaemonSettings::default();
    let old = temp.path().join("old");
    let old_bin = package(&old, "0.152.0");
    prepare_from_package(&daemon, &settings, Some(&old), &old_bin)
        .await
        .expect("seed");

    let standalone = home.join("packages/app-server-daemon");
    let selected = std::fs::canonicalize(standalone.join("current")).expect("selected");
    assert_eq!(
        std::fs::read(selected.join("xdcode-resources/nested/runtime")).expect("runtime"),
        b"runtime"
    );
    assert!(!standalone.join("auto-update-version").exists());
    assert!(
        selected
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("local-")
    );
    assert!(validate_package(&selected).is_ok());
}

#[tokio::test]
async fn incomplete_source_fails_without_selecting_it() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("package");
    let bin = package(&source, "0.152.0");
    std::fs::remove_file(source.join("bin/xdcode-code-mode-host")).expect("remove helper");
    let home = temp.path().join("home");
    let error = prepare_from_package(
        &daemon(&home),
        &DaemonSettings::default(),
        Some(&source),
        &bin,
    )
    .await
    .expect_err("incomplete package");
    assert!(error.to_string().contains("bin/xdcode-code-mode-host"));
    assert!(!home.join("packages/app-server-daemon/current").exists());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn provisioned_macos_bundle_seeds_from_its_running_executable() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("package");
    let launcher = package(&source, "0.1.0-internal-test.202609091200.1");
    xdcode_utils_cargo_bin::write_executable(&launcher, "#!/bin/sh\necho xdcode 0.0.0\n")
        .expect("launcher");
    let bundle = source.join("XdcodeCLI.app/Contents/MacOS/xdcode");
    std::fs::create_dir_all(bundle.parent().expect("bundle parent")).expect("bundle dir");
    std::fs::write(&bundle, b"provisioned executable").expect("bundle executable");
    let home = temp.path().join("home");
    prepare_from_package(
        &daemon(&home),
        &DaemonSettings::default(),
        Some(&source),
        &bundle,
    )
    .await
    .expect("seed provisioned bundle");
    let selected = std::fs::canonicalize(home.join("packages/app-server-daemon/current"))
        .expect("selected release");
    assert_eq!(
        std::fs::read(selected.join("XdcodeCLI.app/Contents/MacOS/xdcode"))
            .expect("bundled executable"),
        b"provisioned executable"
    );
    assert!(
        !home
            .join("packages/app-server-daemon/auto-update-version")
            .exists()
    );
}

#[tokio::test]
async fn legacy_selection_is_not_migrated_or_refreshed() {
    let temp = tempfile::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let legacy = home.join("packages/standalone");
    let bin = package(&legacy.join("releases/old"), "0.150.0");
    std::os::unix::fs::symlink("releases/old", legacy.join("current")).unwrap();
    let state = home.join("app-server-daemon");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("app-server.stderr.log"), b"").unwrap();
    let source = temp.path().join("new");
    let new_bin = package(&source, "0.160.0");
    prepare_from_package(
        &daemon(&home),
        &DaemonSettings::default(),
        Some(&source),
        &new_bin,
    )
    .await
    .unwrap();
    assert_eq!(
        crate::managed_install::managed_xdcode_bin(&home)
            .canonicalize()
            .unwrap(),
        bin.canonicalize().unwrap()
    );
    assert!(!home.join("packages/app-server-daemon").exists());
}

#[tokio::test]
async fn standalone_seed_never_enables_updates_even_for_latest_channel() {
    for follows_latest in [false, true] {
        let temp = tempfile::TempDir::new().unwrap();
        let home = temp.path().join("home");
        let standalone = home.join("packages/standalone");
        let source = standalone.join("releases/0.152.0-local-target");
        let bin = package(&source, "0.152.0");
        std::os::unix::fs::symlink(&source, standalone.join("current")).unwrap();
        if follows_latest {
            std::fs::write(
                standalone.join("auto-update-version"),
                "0.152.0-local-target",
            )
            .unwrap();
        }
        prepare_from_package(
            &daemon(&home),
            &DaemonSettings::default(),
            Some(&source),
            &bin,
        )
        .await
        .unwrap();
        let root = home.join("packages/app-server-daemon");
        assert!(root.join("current").canonicalize().is_ok());
        assert!(!root.join("auto-update-version").exists());
    }
}

#[tokio::test]
async fn explicit_package_update_is_unsupported_without_confirmation() {
    let error = super::update_from_cli(|_| panic!("must not prompt or modify an installation"))
        .await
        .expect_err("updates are unsupported");
    assert!(error.to_string().contains("unsupported in xdcode"));
}

#[tokio::test]
async fn stopped_daemon_follows_manually_selected_cli_and_retains_old_package() {
    let temp = tempfile::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("source");
    let binary = package(&source, "0.152.0");
    let settings = DaemonSettings::default();
    prepare_from_package(&daemon(&home), &settings, Some(&source), &binary)
        .await
        .unwrap();
    let selected = home
        .join("packages/app-server-daemon/current")
        .canonicalize()
        .unwrap();
    let contents = std::fs::read(selected.join("bin/xdcode")).unwrap();
    package(&source, "0.160.0");
    prepare_from_package(&daemon(&home), &settings, Some(&source), &binary)
        .await
        .unwrap();
    let new_selection = home
        .join("packages/app-server-daemon/current")
        .canonicalize()
        .unwrap();
    assert_ne!(new_selection, selected);
    assert_eq!(
        std::fs::read(new_selection.join("bin/xdcode")).unwrap(),
        std::fs::read(&binary).unwrap()
    );
    assert_eq!(
        std::fs::read(selected.join("bin/xdcode")).unwrap(),
        contents
    );
    prepare_from_package(&daemon(&home), &settings, Some(&source), &binary)
        .await
        .unwrap();
    assert_eq!(
        home.join("packages/app-server-daemon/current")
            .canonicalize()
            .unwrap(),
        new_selection
    );
}

#[tokio::test]
async fn broken_selection_is_not_a_missing_installation() {
    let temp = tempfile::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("source");
    let bin = package(&source, "0.152.0");
    let daemon = daemon(&home);
    let current = home.join("packages/app-server-daemon/current");
    std::fs::create_dir_all(current.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("missing-release", &current).unwrap();
    let error = prepare_from_package(&daemon, &DaemonSettings::default(), Some(&source), &bin)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("repair the existing installation")
    );
    assert_eq!(
        std::fs::read_link(current).unwrap(),
        PathBuf::from("missing-release")
    );
}
