//! Exercise native initialization with real prepared libraries in a relocated package.

use std::fs;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use tokio::process::Command;
use tokio::time::timeout;
use xdcode_install_context::InstallContext;
use xdcode_realtime_webrtc::VoiceHost;
use xdcode_utils_cargo_bin::cargo_bin;

const DEADLINE: Duration = Duration::from_secs(/*secs*/ 10);

#[tokio::test]
#[ignore = "requires XDCODE_TEST_VOICE_RUNTIME containing real prepared native libraries"]
async fn relocated_runtime_initializes_closes_and_rejects_duplicate_initialization() -> Result<()> {
    let source = std::env::var_os("XDCODE_TEST_VOICE_RUNTIME")
        .context("set XDCODE_TEST_VOICE_RUNTIME to a matching prepared runtime")?;
    let source = fs::canonicalize(source)?;
    ensure!(
        source.join("runtime.json").is_file(),
        "prepared runtime receipt missing"
    );
    let directory = tempfile::Builder::new()
        .prefix("voice native package ")
        .tempdir()?;
    let root = directory.path().join("staging");
    let runtime = root.join("xdcode-resources/voice");
    fs::create_dir_all(&runtime)?;
    let mut pending = vec![(source, runtime.clone())];
    while let Some((source, destination)) = pending.pop() {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                fs::create_dir(&target)?;
                pending.push((entry.path(), target));
            } else {
                ensure!(kind.is_file(), "runtime inputs must be physical files");
                fs::copy(entry.path(), target)?;
            }
        }
    }
    let helper_source = cargo_bin("xdcode-voice-host")?;
    let name = helper_source.file_name().context("helper filename")?;
    fs::create_dir_all(runtime.join("bin"))?;
    xdcode_utils_cargo_bin::copy_executable(&helper_source, &runtime.join("bin").join(name))?;
    fs::create_dir(root.join("bin"))?;
    let app_name = if cfg!(windows) {
        "xdcode.exe"
    } else {
        "xdcode"
    };
    fs::write(root.join("bin").join(app_name), [])?;
    fs::write(root.join("xdcode-package.json"), "{}")?;
    let moved = directory.path().join("relocated package");
    fs::rename(root, &moved)?;
    let package = InstallContext::from_exe(
        /*is_macos*/ cfg!(target_os = "macos"),
        Some(&moved.join("bin").join(app_name)),
        /*method_override*/ None,
    )
    .package_layout
    .context("package layout")?;
    let output = timeout(
        DEADLINE,
        Command::new(&helper_source)
            .arg("--build-commit")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(output.status.success(), "helper build identity failed");
    let build_commit = String::from_utf8(output.stdout)?.trim().to_owned();
    VoiceHost::connect(&package, &build_commit)
        .await?
        .initialize_runtime()
        .await?
        .close()
        .await?;

    let host = VoiceHost::connect(&package, &build_commit)
        .await?
        .initialize_runtime()
        .await?;
    assert!(host.initialize_runtime().await.is_err());
    Ok(())
}
