//! The configured executable must not create a new Xdcode registry when pointed at the wrong directory.

#[test]
fn requires_an_existing_registry_without_creating_databases() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let output = xdcode_utils_process::background_command(xdcode_utils_cargo_bin::cargo_bin(
        "xdcode-message-board-mcp",
    )?)
    .env("XDCODE_MESSAGE_BOARD_TOKEN", "startup-test-only-token")
    .arg("--sqlite-home")
    .arg(directory.path())
    .args(["--listen", "127.0.0.1:0"])
    .output()?;
    assert!(!output.status.success());
    assert!(directory.path().read_dir()?.next().is_none());
    Ok(())
}
