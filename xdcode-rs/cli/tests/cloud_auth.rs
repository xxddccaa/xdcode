use anyhow::Result;
use tempfile::TempDir;
use wiremock::MockServer;

#[tokio::test]
async fn removed_cloud_commands_cannot_contact_configured_endpoint() -> Result<()> {
    let server = MockServer::start().await;
    let home = TempDir::new()?;
    for args in [
        vec!["cloud", "list", "--json"],
        vec!["cloud-tasks", "list"],
        vec!["remote-control", "pair"],
        vec!["apply", "task-id"],
    ] {
        assert_cmd::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", home.path())
            .env("XDCODE_CLOUD_TASKS_BASE_URL", server.uri())
            .args(args)
            .timeout(std::time::Duration::from_secs(5))
            .assert()
            .failure();
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    Ok(())
}
