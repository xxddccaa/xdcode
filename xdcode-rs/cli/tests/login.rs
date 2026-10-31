use anyhow::Result;
use tempfile::TempDir;

#[test]
fn official_login_flags_are_rejected_without_storing_credentials() -> Result<()> {
    let home = TempDir::new()?;
    for args in [
        vec!["login", "--with-api-key"],
        vec!["login", "--device-auth"],
        vec!["login", "status"],
        vec!["logout", "--json"],
    ] {
        assert_cmd::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", home.path())
            .args(args)
            .timeout(std::time::Duration::from_secs(5))
            .assert()
            .failure();
        assert!(!home.path().join("auth.json").exists());
    }
    Ok(())
}
