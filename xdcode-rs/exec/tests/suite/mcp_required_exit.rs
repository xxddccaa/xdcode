use core_test_support::responses;
use core_test_support::test_xdcode_exec::test_xdcode_exec;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use std::fs;
use xdcode_core::config::set_project_trust_level;
use xdcode_protocol::config_types::TrustLevel;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exits_non_zero_when_required_mcp_server_fails_to_initialize() -> anyhow::Result<()> {
    let test = test_xdcode_exec();

    let config_toml = r#"
        [mcp_servers.required_broken]
        command = "xdcode-definitely-not-a-real-binary"
        required = true
    "#;
    let user_config = test.home_path().join("config.toml");
    fs::write(&user_config, config_toml)?;
    let project = test.cwd_path().join("repo");
    let cwd = project.join("subdir");
    fs::create_dir_all(&cwd)?;
    fs::create_dir(project.join(".xdcode"))?;
    fs::create_dir(project.join(".git"))?;
    fs::write(project.join(".git/HEAD"), "ref: refs/heads/main\n")?;

    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, responses::sse_completed("resp_1")).await;
    let run = |args: &[&str]| {
        test.cmd_with_server(&server)
            .arg("--cd")
            .arg(&cwd)
            .args(["--ephemeral", "--experimental-json"])
            .args(args)
            .arg("hello")
            .assert()
    };
    let expect_mcp_failure = |args: &[&str]| {
        run(args).code(1).stderr(contains(
            "required MCP servers failed to initialize: required_broken",
        ));
    };

    expect_mcp_failure(&[]);
    fs::rename(&user_config, project.join(".xdcode/config.toml"))?;
    let first = run(&["--sandbox", "workspace-write"]).success();
    let stderr = std::str::from_utf8(&first.get_output().stderr)?;
    let warning = stderr
        .lines()
        .find(|line| line.contains("xdcode exec --trust"))
        .expect("project trust warning");
    insta::assert_snapshot!(warning, @"warning: Untrusted project-local configuration is disabled. Run `xdcode exec --trust` to allow project configuration and MCP servers for this and future chats in the project, including MCP commands outside the sandbox. Explicitly untrusted projects must have their trust setting updated first.");
    expect_mcp_failure(&["--sandbox", "read-only", "--trust"]);
    expect_mcp_failure(&[]);

    set_project_trust_level(test.home_path(), &project, TrustLevel::Untrusted)?;
    let refusal = fs::read_to_string(&user_config)?;
    run(&["--trust"])
        .failure()
        .stderr(contains("is marked as untrusted"));
    run(&["--ignore-user-config", "resume", "--last", "--trust"])
        .failure()
        .stderr(contains(
            "--trust cannot be combined with --ignore-user-config",
        ));
    assert_eq!(fs::read_to_string(user_config)?, refusal);
    Ok(())
}
