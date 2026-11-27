use anyhow::Result;
use tempfile::TempDir;

#[test]
fn official_updater_commands_are_rejected() -> Result<()> {
    let home = TempDir::new()?;
    for args in [
        vec!["update", "--yes"],
        vec!["app-server", "daemon", "update"],
        vec!["app-server", "daemon", "pid-update-loop"],
        vec!["app-server", "daemon", "enable-remote-control"],
        vec!["app-server", "daemon", "disable-remote-control"],
    ] {
        assert_cmd::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", home.path())
            .args(args)
            .timeout(std::time::Duration::from_secs(5))
            .assert()
            .failure();
    }
    // Only the launcher's scratch `tmp/` (arg0 helper links) may appear; no update state.
    for entry in std::fs::read_dir(home.path())? {
        assert_eq!(entry?.file_name(), "tmp");
    }
    Ok(())
}
