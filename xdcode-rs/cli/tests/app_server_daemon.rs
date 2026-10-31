#![cfg(unix)]

use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;
use xdcode_utils_cargo_bin::copy_executable;

struct TestDaemon {
    home: TempDir,
    xdcode: PathBuf,
    unmanaged: Option<Child>,
}

impl TestDaemon {
    fn new() -> Result<Self> {
        let home = tempfile::Builder::new().tempdir_in("/tmp")?;
        let xdcode = xdcode_utils_cargo_bin::cargo_bin("xdcode")?;
        let xdcode_source = std::fs::canonicalize(&xdcode)?;
        let target = if cfg!(target_os = "macos") {
            format!("{}-apple-darwin", std::env::consts::ARCH)
        } else {
            format!("{}-unknown-linux-musl", std::env::consts::ARCH)
        };
        let standalone = home.path().join("packages/standalone");
        let release_name = format!("0.0.0-{target}");
        let managed = standalone
            .join("releases")
            .join(&release_name)
            .join("bin/xdcode");
        std::fs::create_dir_all(managed.parent().context("managed bin parent")?)?;
        // Preserve the installed path without invalidating the shared CLI's Rosetta cache.
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            copy_executable(&xdcode_source, &managed)?;
            // Translate the fixture before timed daemon capability and readiness checks.
            ensure!(
                Command::new(&managed)
                    .env("XDCODE_HOME", home.path())
                    .arg("--version")
                    .output()?
                    .status
                    .success(),
                "failed to prepare managed test executable"
            );
        }
        #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
        if std::fs::hard_link(&xdcode_source, &managed).is_err() {
            copy_executable(&xdcode_source, &managed)?;
        }
        std::fs::write(standalone.join("auto-update-version"), &release_name)?;
        std::os::unix::fs::symlink(
            PathBuf::from("releases").join(release_name),
            standalone.join("current"),
        )?;
        // Model a daemon that was previously launched and is currently stopped.
        let state = home.path().join("app-server-daemon");
        std::fs::create_dir(&state)?;
        std::fs::write(state.join("app-server.stderr.log"), b"")?;
        Ok(Self {
            home,
            xdcode,
            unmanaged: None,
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.xdcode);
        command.env("XDCODE_HOME", self.home.path());
        command
    }

    fn lifecycle(&self, action: &str) -> Result<Value> {
        let output = self
            .command()
            .args(["app-server", "daemon", action])
            .output()?;
        ensure!(
            output.status.success(),
            "daemon {action} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    fn pid(&self, name: &str) -> Result<u32> {
        let record = std::fs::read(self.home.path().join("app-server-daemon").join(name))
            .with_context(|| format!("failed to read {name}"))?;
        Ok(serde_json::from_slice::<Value>(&record)?["pid"]
            .as_u64()
            .context("pid missing")? as u32)
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(mut child) = self.unmanaged.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.lifecycle("stop");
        if let Ok(pid) = self.pid("app-server-updater.pid") {
            let _ = signal(pid, libc::SIGTERM);
        }
    }
}

#[test]
fn managed_daemon_restarts_from_deleted_working_directory() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let project = daemon.home.path().join("project");
    std::fs::create_dir(&project)?;
    let started = daemon
        .command()
        .current_dir(&project)
        .args(["app-server", "daemon", "start"])
        .output()?;
    ensure!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let old_pid = daemon.pid("app-server.pid")?;

    // Delete the restart caller's cwd without changing the test process's cwd.
    let restarted = Command::new("/bin/sh")
        .args([
            "-c",
            "rmdir \"$1\" && exec \"$2\" app-server daemon restart",
            "restart",
        ])
        .arg(&project)
        .arg(&daemon.xdcode)
        .current_dir(&project)
        .env("XDCODE_HOME", daemon.home.path())
        .output()?;
    ensure!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    let restarted: Value = serde_json::from_slice(&restarted.stdout)?;
    assert_eq!(restarted["status"], "restarted");
    assert_ne!(daemon.pid("app-server.pid")?, old_pid);
    assert_eq!(daemon.lifecycle("version")?["status"], "running");
    assert!(
        !daemon
            .home
            .path()
            .join("app-server-daemon/app-server-updater.pid")
            .exists()
    );
    Ok(())
}

fn signal(pid: u32, signal: libc::c_int) -> Result<()> {
    let raw_pid = libc::pid_t::try_from(pid).context("pid out of range")?;
    ensure!(
        unsafe { libc::kill(raw_pid, signal) } == 0,
        "failed to signal pid {pid}: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

fn wait_for_exit(pid: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let output = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
            .context("failed to invoke ps")?;
        let state = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || state.trim().starts_with('Z') {
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "pid {pid} did not exit: {state}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn managed_identity_survives_locale_and_timezone_changes() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let mut original = Vec::new();
    let pid_file = daemon.home.path().join("app-server-daemon/app-server.pid");
    for (action, locale, timezone, expected_status) in [
        ("start", "C", "UTC0", "started"),
        ("version", "en_AU.UTF-8", "PST8PDT", "running"),
        ("restart", "en_AU.UTF-8", "PST8PDT", "restarted"),
        ("stop", "C", "UTC0", "stopped"),
    ] {
        let output = daemon
            .command()
            .args(["app-server", "daemon", action])
            .env("LC_ALL", locale)
            .env("TZ", timezone)
            .output()?;
        ensure!(
            output.status.success(),
            "daemon {action} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(output["status"], expected_status);
        if action == "start" {
            original = std::fs::read(&pid_file)?;
            let record: Value = serde_json::from_slice(&original)?;
            assert!(record["processIdentity"].is_object());
            // Older clients still receive the original, unnormalized ps value.
            let legacy = Command::new("ps")
                .args([
                    "-p",
                    &daemon.pid("app-server.pid")?.to_string(),
                    "-o",
                    "lstart=",
                ])
                .env("LC_ALL", locale)
                .env("TZ", timezone)
                .output()?;
            ensure!(legacy.status.success(), "failed to read legacy start time");
            assert_eq!(
                record["processStartTime"],
                String::from_utf8(legacy.stdout)?.trim()
            );
        } else if action == "version" {
            assert_eq!(output["backend"], "pid");
            assert_eq!(std::fs::read(&pid_file)?, original);

            let mut legacy: Value = serde_json::from_slice(&original)?;
            legacy
                .as_object_mut()
                .context("PID record object")?
                .remove("processIdentity");
            let legacy_bytes = serde_json::to_vec(&legacy)?;
            std::fs::write(&pid_file, &legacy_bytes)?;
            let result = daemon
                .command()
                .args(["app-server", "daemon", "version"])
                .env("LC_ALL", locale)
                .env("TZ", timezone)
                .output();
            let preserved = std::fs::read(&pid_file);
            // Restore the native identity before assertions so Drop can stop the daemon.
            std::fs::write(&pid_file, &original)?;
            let result = result?;
            assert!(!result.status.success());
            assert_eq!(preserved?, legacy_bytes);
            let stderr =
                String::from_utf8(result.stderr)?.replace(&legacy["pid"].to_string(), "[PID]");
            insta::assert_snapshot!(stderr, @"Error: cannot verify pid-managed process [PID]: legacy start time changed; PID record retained. Retry with the locale and timezone used to start the daemon. If the system clock changed, stop the original process before restarting the daemon");
        }
        if action == "restart" {
            assert!(!pid_file.with_file_name("app-server-updater.pid").exists());
        }
    }
    assert!(!pid_file.exists());
    Ok(())
}

#[test]
fn managed_start_succeeds_when_updater_record_is_invalid() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let state_dir = daemon.home.path().join("app-server-daemon");
    std::fs::create_dir_all(&state_dir)?;
    std::fs::write(state_dir.join("app-server-updater.pid"), "not a PID record")?;

    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let server_pid = daemon.pid("app-server.pid")?;
    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, server_pid);
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn unmanaged_app_server_does_not_launch_updater() -> Result<()> {
    let mut daemon = TestDaemon::new()?;
    daemon.unmanaged = Some(
        daemon
            .command()
            .args(["app-server", "--listen", "unix://"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !daemon
        .command()
        .args(["app-server", "daemon", "version"])
        .output()?
        .status
        .success()
    {
        ensure!(Instant::now() < deadline, "app server did not become ready");
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = daemon.lifecycle("start")?;
    assert_eq!(output["status"], "alreadyRunning");
    assert_eq!(output["backend"], Value::Null);
    assert!(
        !daemon
            .home
            .path()
            .join("app-server-daemon/app-server-updater.pid")
            .exists()
    );
    Ok(())
}
