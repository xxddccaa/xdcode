use anyhow::Result;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;
use xdcode_config::MarketplaceConfigUpdate;
use xdcode_config::record_user_marketplace;
use xdcode_core_plugins::installed_marketplaces::marketplace_install_root;
use xdcode_utils_absolute_path::canonicalize_existing_preserving_symlinks;

fn xdcode_command(xdcode_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?);
    cmd.env("XDCODE_HOME", xdcode_home);
    Ok(cmd)
}

fn configured_marketplace_update() -> MarketplaceConfigUpdate<'static> {
    MarketplaceConfigUpdate {
        source_type: "git",
        source: "https://github.com/owner/repo.git",
        ref_name: Some("main"),
        sparse_paths: &[],
    }
}

fn write_installed_marketplace(xdcode_home: &Path, marketplace_name: &str) -> Result<()> {
    let root = marketplace_install_root(xdcode_home).join(marketplace_name);
    std::fs::create_dir_all(root.join(".agents/plugins"))?;
    std::fs::write(root.join(".agents/plugins/marketplace.json"), "{}")?;
    std::fs::write(root.join("marker.txt"), "installed")?;
    Ok(())
}

#[tokio::test]
async fn marketplace_remove_deletes_config_and_installed_root() -> Result<()> {
    let xdcode_home = TempDir::new()?;
    record_user_marketplace(
        xdcode_home.path(),
        "debug",
        &configured_marketplace_update(),
    )?;
    write_installed_marketplace(xdcode_home.path(), "debug")?;

    xdcode_command(xdcode_home.path())?
        .args(["plugin", "marketplace", "remove", "debug"])
        .assert()
        .success()
        .stdout(contains("Removed marketplace `debug`."));

    let config_path = xdcode_home.path().join("config.toml");
    let config = std::fs::read_to_string(config_path)?;
    assert!(!config.contains("[marketplaces.debug]"));
    assert!(
        !marketplace_install_root(xdcode_home.path())
            .join("debug")
            .exists()
    );
    Ok(())
}

#[tokio::test]
async fn marketplace_remove_json_prints_remove_outcome() -> Result<()> {
    let xdcode_home = TempDir::new()?;
    record_user_marketplace(
        xdcode_home.path(),
        "debug",
        &configured_marketplace_update(),
    )?;
    write_installed_marketplace(xdcode_home.path(), "debug")?;
    let installed_root = marketplace_install_root(xdcode_home.path()).join("debug");
    let normalized_installed_root = canonicalize_existing_preserving_symlinks(&installed_root)?;

    let assert = xdcode_command(xdcode_home.path())?
        .args(["plugin", "marketplace", "remove", "debug", "--json"])
        .assert()
        .success();
    let stdout = assert.get_output().stdout.as_slice();
    let actual: serde_json::Value = serde_json::from_slice(stdout)?;

    assert_eq!(
        actual,
        json!({
            "marketplaceName": "debug",
            "installedRoot": normalized_installed_root.display().to_string(),
        })
    );

    Ok(())
}

#[tokio::test]
async fn marketplace_remove_rejects_unknown_marketplace() -> Result<()> {
    let xdcode_home = TempDir::new()?;

    xdcode_command(xdcode_home.path())?
        .args(["plugin", "marketplace", "remove", "debug"])
        .assert()
        .failure()
        .stderr(contains(
            "marketplace `debug` is not configured or installed",
        ));

    Ok(())
}

#[tokio::test]
async fn marketplace_remove_preserves_marketplace_referenced_by_session_config() -> Result<()> {
    let xdcode_home = TempDir::new()?;
    record_user_marketplace(
        xdcode_home.path(),
        "debug",
        &configured_marketplace_update(),
    )?;
    write_installed_marketplace(xdcode_home.path(), "debug")?;
    let config_path = xdcode_home.path().join("config.toml");
    let config = std::fs::read_to_string(&config_path)?;

    xdcode_command(xdcode_home.path())?
        .args([
            "plugin", "marketplace",
            "-c", "marketplaces.debug.source_type=\"git\"",
            "remove", "debug",
        ])
        .assert()
        .failure()
        .stderr(contains(
            "marketplace `debug` is configured in session-flags; remove it from that configuration source instead",
        ));

    assert_eq!(std::fs::read_to_string(config_path)?, config);
    assert_eq!(
        std::fs::read_to_string(
            marketplace_install_root(xdcode_home.path()).join("debug/marker.txt")
        )?,
        "installed",
    );
    Ok(())
}
