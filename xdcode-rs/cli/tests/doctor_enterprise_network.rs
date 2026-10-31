#[cfg(target_os = "macos")]
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Context as _;
use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use pretty_assertions::assert_eq;
use regex_lite::Regex;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use xdcode_config::types::AuthCredentialsStoreMode;
use xdcode_state::SqliteConfig;
use xdcode_utils_absolute_path::AbsolutePathBuf;

async fn local_provider() -> Result<(MockServer, TempDir)> {
    let server = MockServer::start().await;
    for (request_method, request_path, status) in
        [("HEAD", "/v1/responses", 200), ("GET", "/v1/models", 401)]
    {
        Mock::given(method(request_method))
            .and(path(request_path))
            .respond_with(ResponseTemplate::new(status).insert_header("x-request-id", "fixture-id"))
            .mount(&server)
            .await;
    }
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "cli_auth_credentials_store = \"ephemeral\"\ncheck_for_update_on_startup = false\nmodel_provider = \"local\"\n[model_providers.local]\nname = \"local\"\nbase_url = \"{}/v1\"\nwire_api = \"responses\"\n",
            server.uri()
        ),
    )?;
    Ok((server, home))
}

fn local_command(home: &TempDir) -> Result<Command> {
    let mut command = Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?);
    command
        .current_dir(home.path())
        .env("XDCODE_HOME", home.path())
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .stdin(Stdio::null());
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "XDCODE_ACCESS_TOKEN",
        "XDCODE_API_KEY",
        "PROVIDER_API_KEY",
        "XDCODE_CA_CERTIFICATE",
        "SSL_CERT_FILE",
        "PROVIDER_WORKLOAD_IDENTITY_CONTEXT",
        "PROVIDER_IDENTITY_TOKEN_FILE",
        "PROVIDER_FEDERATION_RULE_ID",
    ] {
        command.env_remove(name);
    }
    Ok(command)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_selects_checks_without_database_or_mcp_scans() -> Result<()> {
    let (server, home) = local_provider().await?;
    Mock::given(path("/unrelated-mcp"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let sqlite =
        SqliteConfig::new_for_testing(AbsolutePathBuf::try_from(home.path().to_path_buf())?);
    let database = sqlite.logs_db_path();
    std::fs::write(&database, "not a SQLite database")?;
    let mcp_url = format!("{}/unrelated-mcp", server.uri());
    let output = local_command(&home)?
        .args(["doctor", "--network-only", "--json", "--all", "--feedback"])
        .arg("-c")
        .arg(format!("mcp_servers.unrelated.url={mcp_url:?}"))
        .output()?;
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert!(output.status.success(), "{report:#}");
    let checks = report["checks"].as_object().unwrap();
    assert_eq!(
        checks.keys().map(String::as_str).collect::<Vec<_>>(),
        vec![
            "config.load",
            "network.env",
            "network.provider_reachability",
            "network.websocket_reachability",
        ]
    );
    assert_eq!(std::fs::read_to_string(database)?, "not a SQLite database");
    server.verify().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_reports_recoverable_config_warnings_and_effective_config() -> Result<()> {
    let (_server, home) = local_provider().await?;
    let baseline_output = local_command(&home)?
        .args(["doctor", "--network-only", "--json"])
        .output()?;
    let baseline_report: Value = serde_json::from_slice(&baseline_output.stdout)?;
    assert!(baseline_output.status.success(), "{baseline_report:#}");
    let baseline_warning_count =
        baseline_report["checks"]["config.load"]["details"]["startup warnings"]
            .as_str()
            .unwrap_or("0")
            .parse::<usize>()?;
    let config_path = home.path().join("config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    std::fs::write(
        config_path,
        format!("{config}\n[otel.span_attributes]\n\"\" = \"missing-key\"\n"),
    )?;
    let output = local_command(&home)?
        .args(["doctor", "--network-only", "--json"])
        .output()?;
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert!(output.status.success(), "{report:#}");
    let config_check = &report["checks"]["config.load"];
    let warning_count = config_check["details"]["startup warnings"]
        .as_str()
        .context("missing startup-warning count")?
        .parse::<usize>()?;
    let warnings = &config_check["details"]["startup warning"];
    let startup_warning = std::iter::once(warnings)
        .chain(warnings.as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .find(|warning| warning.starts_with("Ignoring invalid `otel.span_attributes` config"))
        .context("missing recoverable OTEL config warning")?;
    let check_statuses = report["checks"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, check)| (id.clone(), check["status"].clone()))
        .collect::<serde_json::Map<_, _>>();
    assert_eq!(
        json!({
            "overallStatus": report["overallStatus"],
            "checkStatuses": check_statuses,
            "config": {
                "summary": config_check["summary"],
                "scope": config_check["details"]["configuration scope"],
                "provider": config_check["details"]["model provider"],
                "startupWarnings": warning_count,
                "startupWarning": startup_warning,
            },
        }),
        json!({
            "overallStatus": "warning",
            "checkStatuses": {
                "config.load": "warning",
                "network.env": "ok",
                "network.provider_reachability": "ok",
                "network.websocket_reachability": "ok",
            },
            "config": {
                "summary": "config loaded",
                "scope": "local invocation config",
                "provider": "local",
                "startupWarnings": baseline_warning_count + 1,
                "startupWarning": "Ignoring invalid `otel.span_attributes` config: configured span attribute key must not be empty",
            },
        })
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_config_failure_does_not_guess_probe_targets() -> Result<()> {
    let proxy = MockServer::start().await;
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model_provider = sk-config-credential-fixture\n",
    )?;
    let output = local_command(&home)?
        .args(["doctor", "--network-only", "--json"])
        .env("HTTP_PROXY", proxy.uri())
        .env("HTTPS_PROXY", proxy.uri())
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sk-config-credential-fixture"));
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["checks"].as_object().unwrap().len(), 1);
    assert_eq!(report["checks"]["config.load"]["status"], "fail");
    assert_eq!(
        report["checks"]["config.load"]["details"]["network diagnostics"],
        "effective network configuration is unavailable"
    );
    assert!(proxy.received_requests().await.unwrap().is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_human_output() -> Result<()> {
    let (_server, home) = local_provider().await?;
    let output = local_command(&home)?
        .args([
            "doctor",
            "--network-only",
            "--summary",
            "--ascii",
            "--no-color",
        ])
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    assert!(output.status.success(), "{stdout}");
    insta::assert_snapshot!("network_only_human_output", stdout);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_config_failure_human_output() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model_provider = sk-config-credential-fixture\n",
    )?;
    let output = local_command(&home)?
        .args(["doctor", "--network-only", "--all", "--ascii", "--no-color"])
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(output.status.code(), Some(1));
    assert!(!stdout.contains("sk-config-credential-fixture"));
    // Config loading may canonicalize the directory, rendering may shorten it,
    // and Windows uses backslashes.
    // Normalize the rendered config-file row while retaining its label and filename.
    let file_row = Regex::new(r"(?m)^(      file +).+[\\/]config\.toml$")?;
    assert!(file_row.is_match(&stdout), "{stdout}");
    let stdout = file_row.replace_all(&stdout, "${1}<XDCODE_HOME>/config.toml");
    insta::assert_snapshot!("network_only_config_failure_human_output", stdout);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_uses_local_project_config_without_hosted_policy_or_auth_requests() -> Result<()> {
    let (provider, home) = local_provider().await?;
    let hosted = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&hosted)
        .await;
    let workspace = TempDir::new()?;
    std::fs::create_dir(workspace.path().join(".xdcode"))?;
    let config_path = home.path().join("config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    std::fs::write(
        &config_path,
        format!(
            "chatgpt_base_url = \"{}/backend-api\"\n{config}\n[projects.{:?}]\ntrust_level = \"trusted\"\n",
            hosted.uri(),
            workspace.path().display().to_string()
        ),
    )?;
    std::fs::write(
        workspace.path().join(".xdcode/config.toml"),
        "model = \"project-fixture\"\n",
    )?;
    write_chatgpt_auth(
        home.path(),
        ChatGptAuthFixture::new("private-unused-hosted-token"),
        AuthCredentialsStoreMode::File,
    )?;
    let output = local_command(&home)?
        .arg("--cd")
        .arg(workspace.path())
        .args(["doctor", "--network-only", "--json"])
        .output()?;
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert!(output.status.success(), "{report:#}");
    assert_eq!(
        report["checks"]["config.load"]["details"]["model"],
        "project-fixture"
    );
    assert_eq!(
        report["checks"]["config.load"]["details"]["configuration scope"],
        "local invocation config"
    );
    assert!(hosted.received_requests().await.unwrap().is_empty());
    assert!(
        provider
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.url.path().starts_with("/v1/"))
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-unused-hosted-token"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_does_not_follow_unconfigured_redirects() -> Result<()> {
    let configured = MockServer::start().await;
    let unconfigured = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "location",
            format!("{}/private-redirect", unconfigured.uri()),
        ))
        .mount(&configured)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "location",
            format!("{}/private-redirect", unconfigured.uri()),
        ))
        .mount(&configured)
        .await;
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "model_provider = \"local\"\n[model_providers.local]\nname = \"local\"\nbase_url = \"{}/v1\"\nwire_api = \"responses\"\n",
            configured.uri()
        ),
    )?;
    for extra in [vec!["--json"], vec!["--ascii", "--no-color"]] {
        let output = local_command(&home)?
            .args(["doctor", "--network-only"])
            .args(extra)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(unconfigured.received_requests().await.unwrap().is_empty());
    }
    assert_eq!(configured.received_requests().await.unwrap().len(), 4);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_only_redacts_rejected_websocket_evidence_in_both_formats() -> Result<()> {
    let (server, home) = local_provider().await?;
    let secret = "private-wire-credential";
    Mock::given(method("GET"))
        .and(path("/v1/responses"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {secret}"),
        ))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_string(secret)
                .insert_header("x-request-id", secret)
                .insert_header("set-cookie", secret),
        )
        .expect(2)
        .mount(&server)
        .await;
    let config_path = home.path().join("config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    std::fs::write(
        &config_path,
        format!(
            "{config}supports_websockets = true\nenv_key = \"XDCODE_DOCTOR_FIXTURE_KEY\"\n[model_providers.local.query_params]\nkey = \"{secret}\"\n"
        ),
    )?;
    for json_output in [true, false] {
        let mut command = local_command(&home)?;
        command
            .args(["doctor", "--network-only", "--all", "--ascii", "--no-color"])
            .env("XDCODE_DOCTOR_FIXTURE_KEY", secret);
        if json_output {
            command.arg("--json");
        }
        let output = command.output()?;
        let stdout = String::from_utf8(output.stdout)?;
        assert!(output.status.success(), "{stdout}");
        assert!(!stdout.contains(secret), "{stdout}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        assert!(stdout.contains("HTTP upgrade rejected"), "{stdout}");
        assert!(!stdout.contains("set-cookie"));
        if json_output {
            let report: Value = serde_json::from_str(&stdout)?;
            let websocket = &report["checks"]["network.websocket_reachability"];
            assert_eq!(websocket["status"], "warning");
            assert_eq!(
                websocket["details"]["handshake result"],
                "HTTP 403 Forbidden"
            );
            assert_eq!(websocket["details"]["upgrade x-request-id"], "present");
            assert_eq!(report["checks"].as_object().unwrap().len(), 4);
        }
    }
    server.verify().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_custom_ca_falls_back_to_system_roots() -> Result<()> {
    let server = MockServer::start().await;
    for (request_method, request_path) in [("HEAD", "/v1/responses"), ("GET", "/v1/models")] {
        Mock::given(method(request_method))
            .and(path(request_path))
            .respond_with(ResponseTemplate::new(200))
            .expect(2)
            .mount(&server)
            .await;
    }

    let xdcode_home = TempDir::new()?;
    let certificate = xdcode_home.path().join("invalid-ca.pem");
    std::fs::write(&certificate, "not a certificate")?;
    std::fs::write(
        xdcode_home.path().join("config.toml"),
        format!(
            "model_provider = \"local\"\n[model_providers.local]\nname = \"local\"\nbase_url = \"{}/v1\"\nwire_api = \"responses\"\n",
            server.uri()
        ),
    )?;
    for sandbox in [None, Some("seatbelt")] {
        let mut command = Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?);
        command
            .args(["doctor", "--json"])
            .env("XDCODE_HOME", xdcode_home.path())
            .env("XDCODE_CA_CERTIFICATE", &certificate)
            .stdin(Stdio::null());
        if let Some(sandbox) = sandbox {
            command
                .env("XDCODE_SANDBOX", sandbox)
                .env("HTTP_PROXY", "http://127.0.0.1:1")
                .env("http_proxy", "http://127.0.0.1:1")
                .env("HTTPS_PROXY", "http://127.0.0.1:1")
                .env("https_proxy", "http://127.0.0.1:1")
                .env("NO_PROXY", "")
                .env("no_proxy", "");
        } else {
            command
                .env("NO_PROXY", "127.0.0.1,localhost")
                .env("no_proxy", "127.0.0.1,localhost");
        }
        let output = command
            .output()
            .context("failed to run the doctor with an invalid custom CA")?;
        let report: Value = serde_json::from_slice(&output.stdout)?;

        assert!(
            report["checks"]["network.provider_reachability"]["details"]["local API inference URL"]
                .as_str()
                .is_some_and(|detail| detail.ends_with("reachable (HTTP 200)"))
        );
    }
    server.verify().await;

    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn doctor_reports_macos_system_proxy_configuration_and_policy() -> Result<()> {
    let xdcode_home = TempDir::new()?;
    let report = doctor_report(xdcode_home.path())?;
    let details = &report["checks"]["network.env"]["details"];

    assert_eq!(details["respect system proxy"], json!("disabled"));
    assert!(matches!(
        details["system proxy"].as_str(),
        Some("automatic (PAC)" | "manual" | "direct" | "unavailable")
    ));

    std::fs::write(
        xdcode_home.path().join("config.toml"),
        "[features]\nrespect_system_proxy = true\n",
    )?;
    let report = doctor_report(xdcode_home.path())?;
    assert_eq!(
        report["checks"]["network.env"]["details"]["respect system proxy"],
        json!("enabled")
    );

    Ok(())
}

#[cfg(target_os = "macos")]
fn doctor_report(xdcode_home: &Path) -> Result<Value> {
    let output = Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
        .args(["doctor", "--json"])
        .env("XDCODE_HOME", xdcode_home)
        .stdin(Stdio::null())
        .output()
        .context("failed to run the doctor")?;

    serde_json::from_slice(&output.stdout).context("doctor did not emit a valid json report")
}
