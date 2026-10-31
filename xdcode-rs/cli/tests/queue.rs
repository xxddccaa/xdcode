use std::path::Path;
use std::process::Output;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

const THREAD_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

fn configure_queue_home(xdcode_home: &Path) -> Result<()> {
    MockResponsesConfig::new("http://127.0.0.1:9")
        .with_provider_config(
            "supports_websockets = false\nexperimental_bearer_token = \"unused-queue-client-fixture-key\"",
        )
        .write(xdcode_home)?;
    Ok(())
}

enum QueueResponse {
    Success,
    ResumeRejected,
    AlreadyLoaded,
    ReadRejected,
    MethodNotFound,
    #[cfg(unix)]
    UnknownMethodVariant,
}

#[cfg(unix)]
fn socket_test_home() -> Result<TempDir> {
    // macOS temporary paths can exceed the Unix socket path limit.
    #[cfg(target_os = "macos")]
    let home = tempfile::tempdir_in("/tmp")?;
    #[cfg(not(target_os = "macos"))]
    let home = TempDir::new()?;
    configure_queue_home(home.path())?;
    Ok(home)
}

async fn respond_to_queue_request<S>(
    stream: S,
    xdcode_home: &Path,
    response: QueueResponse,
) -> Result<Value>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut websocket = accept_async(stream).await?;
    let initialize = websocket
        .next()
        .await
        .context("missing initialize request")??;
    let initialize: Value = serde_json::from_str(initialize.to_text()?)?;
    assert_eq!(initialize["method"], "initialize");
    assert_eq!(
        initialize["params"]["capabilities"]["experimentalApi"],
        true
    );
    let initialized_response = json!({
        "id": initialize["id"],
        "result": {
            "userAgent": "xdcode_cli_rs/0.0.0-test",
            "xdcodeHome": xdcode_home,
        },
    });
    websocket
        .send(Message::Text(initialized_response.to_string().into()))
        .await?;

    let initialized = websocket
        .next()
        .await
        .context("missing initialized notification")??;
    let initialized: Value = serde_json::from_str(initialized.to_text()?)?;
    assert_eq!(initialized["method"], "initialized");

    let request = websocket.next().await.context("missing queue request")??;
    let request: Value = serde_json::from_str(request.to_text()?)?;
    assert_eq!(request["method"], "thread/queue/add");
    assert_eq!(request["params"]["threadId"], THREAD_ID);

    let result = match response {
        QueueResponse::Success
        | QueueResponse::ResumeRejected
        | QueueResponse::AlreadyLoaded
        | QueueResponse::ReadRejected => json!({
            "id": request["id"],
            "result": {
                "queuedSubmission": {
                    "id": "queued-submission-id",
                    "input": request["params"]["input"],
                    "clientUserMessageId": request["params"]["clientUserMessageId"],
                },
            },
        }),
        QueueResponse::MethodNotFound => json!({
            "id": request["id"],
            "error": { "code": -32601, "message": "Method not found" },
        }),
        #[cfg(unix)]
        QueueResponse::UnknownMethodVariant => json!({
            "id": request["id"],
            "error": {
                "code": -32600,
                "message": "Invalid request: unknown variant `thread/queue/add`, expected `thread/list`",
            },
        }),
    };
    websocket
        .send(Message::Text(result.to_string().into()))
        .await?;
    if matches!(response, QueueResponse::ResumeRejected) {
        for (method, mut reply) in [
            (
                "thread/read",
                json!({"result": {"thread": {"status": {"type": "notLoaded"}, "cwd": r"C:\workspace"}}}),
            ),
            (
                "thread/resume",
                json!({"error": {"code": -32603, "message": "thread already has an active writer"}}),
            ),
        ] {
            let rpc = websocket.next().await.context("missing resume request")??;
            let rpc: Value = serde_json::from_str(rpc.to_text()?)?;
            assert_eq!(rpc["method"], method);
            assert_eq!(rpc["params"]["threadId"], THREAD_ID);
            if method == "thread/resume" {
                assert_eq!(rpc["params"]["excludeTurns"], true);
                for key in ["model", "modelProvider", "cwd", "config"] {
                    assert!(rpc["params"][key].is_null(), "unexpected override: {key}");
                }
            }
            reply["id"] = rpc["id"].clone();
            websocket
                .send(Message::Text(reply.to_string().into()))
                .await?;
        }
    }
    if matches!(
        response,
        QueueResponse::AlreadyLoaded | QueueResponse::ReadRejected
    ) {
        let rpc = websocket.next().await.context("missing thread read")??;
        let rpc: Value = serde_json::from_str(rpc.to_text()?)?;
        assert_eq!(rpc["method"], "thread/read");
        assert_eq!(rpc["params"]["threadId"], THREAD_ID);
        assert_ne!(rpc["params"]["includeTurns"], true);
        let mut reply = match response {
            QueueResponse::AlreadyLoaded => json!({
                "result": {"thread": {"status": {"type": "idle"}, "cwd": r"C:\workspace"}},
            }),
            QueueResponse::ReadRejected => json!({
                "error": {"code": -32603, "message": "thread status unavailable"},
            }),
            _ => unreachable!(),
        };
        reply["id"] = rpc["id"].clone();
        websocket
            .send(Message::Text(reply.to_string().into()))
            .await?;
    }
    Ok(request)
}

#[tokio::test]
async fn queue_submits_message_to_remote_app_server() -> Result<()> {
    let (output, request) = run_remote_queue_command(QueueResponse::Success).await?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(request["params"]["threadId"], THREAD_ID);
    assert_eq!(request["params"]["input"][0]["text"], "do the thing");
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("Queued message queued-submission-id for thread {THREAD_ID}.\n")
    );
    Ok(())
}

#[tokio::test]
async fn queue_succeeds_when_another_server_owns_the_thread() -> Result<()> {
    let (output, _) = run_remote_queue_command(QueueResponse::ResumeRejected).await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("Queued message queued-submission-id for thread {THREAD_ID}.\n")
    );
    assert!(String::from_utf8(output.stderr)?.contains("thread already has an active writer"));
    Ok(())
}

#[tokio::test]
async fn queue_resume_leaves_loaded_threads_running() -> Result<()> {
    let (output, _) = run_remote_queue_command(QueueResponse::AlreadyLoaded).await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("Queued message queued-submission-id for thread {THREAD_ID}.\n")
    );
    // The mock disconnects after thread/read, so an extra resume would produce a warning.
    assert!(!String::from_utf8(output.stderr)?.contains("Warning:"));
    Ok(())
}

#[tokio::test]
async fn queue_succeeds_when_resume_status_cannot_be_read() -> Result<()> {
    let (output, _) = run_remote_queue_command(QueueResponse::ReadRejected).await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("Queued message queued-submission-id for thread {THREAD_ID}.\n")
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Warning: message was queued"));
    assert!(stderr.contains("thread status unavailable"));
    Ok(())
}

#[tokio::test]
async fn queue_does_not_fallback_from_unsupported_explicit_remote() -> Result<()> {
    let (output, _) = run_remote_queue_command(QueueResponse::MethodNotFound).await?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)?
            .contains("remote app server does not support thread/queue/add")
    );
    Ok(())
}

#[test]
fn queue_rejects_empty_message() -> Result<()> {
    let output = std::process::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
        .args(["queue", "--thread", THREAD_ID, "--message", ""])
        .output()?;
    assert!(!output.status.success());
    Ok(())
}

#[test]
fn queue_rejects_unsupported_options() -> Result<()> {
    let xdcode_home = TempDir::new()?;
    for (args, expected) in [
        ("queue --image screenshot.png", "image attachments"),
        ("--image=screenshot.png queue", "image attachments"),
        ("queue --model other-model", "`--model`"),
        ("--model other-model queue", "`--model`"),
        ("--oss queue", "`--oss`"),
        ("queue --oss", "`--oss`"),
        ("queue --local-provider ollama", "`--local-provider`"),
        (
            "queue --dangerously-bypass-hook-trust",
            "`--dangerously-bypass-hook-trust`",
        ),
        (
            "-c model=other queue -c analytics.enabled=false",
            "`model` config override",
        ),
        ("queue -s read-only", "`--sandbox`"),
        ("-s read-only queue", "`--sandbox`"),
        ("--approve-for-me queue", "`--approve-for-me`"),
        ("queue --approve-for-me", "`--approve-for-me`"),
        ("--search queue", "`--search`"),
        ("--no-alt-screen queue", "`--no-alt-screen`"),
        ("queue -c model=other", "`model` config override"),
        ("prompt queue", "positional prompt; use `--message`"),
        ("-C . queue --remote unix://", "`--cd` with `--remote`"),
        ("--remote unix:// queue -C .", "`--cd` with `--remote`"),
    ] {
        let output = std::process::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", xdcode_home.path())
            .args(args.split_whitespace())
            .args(["--thread", THREAD_ID, "--message", "do the thing"])
            .output()?;
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr)?;
        assert!(
            stderr.contains("`xdcode queue`") && stderr.contains(expected),
            "{args:?}: {stderr}"
        );
    }
    Ok(())
}

async fn run_remote_queue_command(response: QueueResponse) -> Result<(Output, Value)> {
    let xdcode_home = TempDir::new()?;
    configure_queue_home(xdcode_home.path())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("ws://{}", listener.local_addr()?);
    let remote_first = matches!(response, QueueResponse::MethodNotFound);
    let resume = matches!(
        response,
        QueueResponse::ResumeRejected | QueueResponse::AlreadyLoaded | QueueResponse::ReadRejected
    );
    let server_home = xdcode_home.path().to_path_buf();
    let server = tokio::spawn(async move {
        timeout(DEFAULT_TIMEOUT, async move {
            let (stream, _) = listener.accept().await?;
            respond_to_queue_request(stream, server_home.as_path(), response).await
        })
        .await
        .context("queue mock timed out")?
    });

    let remote_args = if remote_first {
        [
            "--remote",
            endpoint.as_str(),
            "--remote-auth-token-env",
            "XDCODE_REMOTE_TOKEN",
            "queue",
        ]
    } else {
        [
            "queue",
            "--remote",
            endpoint.as_str(),
            "--remote-auth-token-env",
            "XDCODE_REMOTE_TOKEN",
        ]
    };
    let output = timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", xdcode_home.path())
            .env("XDCODE_REMOTE_TOKEN", "test-token")
            .kill_on_drop(true)
            .args(remote_args)
            .args(resume.then_some("--resume"))
            .args(["--thread", THREAD_ID, "--message", "do the thing"])
            .output(),
    )
    .await
    .context("queue command timed out")??;
    let request = server.await?.with_context(|| {
        format!(
            "queue mock failed; CLI stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok((output, request))
}

#[cfg(unix)]
#[tokio::test]
async fn remote_session_commands_with_workload_identity_use_server_auth() -> Result<()> {
    use std::time::Duration;

    use app_test_support::MockResponsesConfig;
    use app_test_support::TestAppServer;
    use app_test_support::create_final_assistant_message_sse_response;
    use core_test_support::responses;
    use tokio::process::Command;
    use tokio::time::timeout;
    use wiremock::Mock;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use xdcode_app_server_client::AppServerEvent;
    use xdcode_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
    use xdcode_app_server_client::RemoteAppServerClient;
    use xdcode_app_server_client::RemoteAppServerConnectArgs;
    use xdcode_app_server_client::RemoteAppServerEndpoint;
    use xdcode_app_server_protocol::ClientRequest;
    use xdcode_app_server_protocol::RequestId;
    use xdcode_app_server_protocol::ServerNotification;
    use xdcode_app_server_protocol::ThreadListParams;
    use xdcode_app_server_protocol::ThreadListResponse;
    use xdcode_app_server_protocol::ThreadQueueListParams;
    use xdcode_app_server_protocol::ThreadQueueListResponse;
    use xdcode_app_server_protocol::ThreadSetNameParams;
    use xdcode_app_server_protocol::ThreadSetNameResponse;
    use xdcode_app_server_protocol::ThreadStartParams;
    use xdcode_app_server_protocol::ThreadStartResponse;
    use xdcode_app_server_protocol::ThreadStatus;
    use xdcode_app_server_protocol::TurnStatus;
    use xdcode_protocol::shell_environment::PROVIDER_FEDERATION_RULE_ID_ENV_VAR;
    use xdcode_protocol::shell_environment::PROVIDER_IDENTITY_TOKEN_FILE_ENV_VAR;

    let xdcode_home = socket_test_home()?;
    let xdcode = xdcode_utils_cargo_bin::cargo_bin("xdcode")?;
    let model = responses::start_mock_server().await;
    let model_requests = Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(responses::sse_response(
            create_final_assistant_message_sse_response("queued message consumed")?,
        ))
        .expect(2)
        .mount_as_scoped(&model)
        .await;
    MockResponsesConfig::new(&model.uri())
        .with_root_config("features.plugins = false\nanalytics.enabled = false")
        .with_provider_config("env_key = \"XDCODE_QUEUE_SERVER_API_KEY\"")
        .write(xdcode_home.path())?;
    let _server = TestAppServer::builder()
        .with_program(&xdcode)
        .with_xdcode_home(xdcode_home.path())
        .with_plugin_startup_tasks()
        .without_managed_config()
        .with_args(&["app-server", "--listen", "unix://"])
        .with_env_overrides(&[
            ("PROVIDER_API_KEY", None),
            ("XDCODE_API_KEY", None),
            ("XDCODE_ACCESS_TOKEN", None),
            (PROVIDER_FEDERATION_RULE_ID_ENV_VAR, None),
            (PROVIDER_IDENTITY_TOKEN_FILE_ENV_VAR, None),
            ("XDCODE_QUEUE_SERVER_API_KEY", Some("server-only-test-key")),
        ])
        .build()
        .await?;
    let socket_path = xdcode_app_server::app_server_control_socket_path(xdcode_home.path())?;
    timeout(Duration::from_secs(30), async {
        while !socket_path.as_path().try_exists()? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;

    let endpoint = format!("unix://{}", socket_path.as_path().display());
    let mut app = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
        endpoint: RemoteAppServerEndpoint::UnixSocket { socket_path },
        client_name: "queue-e2e-test".to_string(),
        client_version: "0.1.0".to_string(),
        experimental_api: true,
        mcp_server_provider_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await?;
    let started: ThreadStartResponse = app
        .request_typed(ClientRequest::ThreadStart {
            request_id: RequestId::Integer(1),
            params: ThreadStartParams::default(),
        })
        .await?;
    let thread_id = started.thread.id;
    let message = "synthetic WIF queue end-to-end message";
    let output = timeout(
        Duration::from_secs(30),
        Command::new(&xdcode)
            .env("XDCODE_HOME", xdcode_home.path())
            .env(PROVIDER_FEDERATION_RULE_ID_ENV_VAR, "rule-test")
            .env(
                PROVIDER_IDENTITY_TOKEN_FILE_ENV_VAR,
                xdcode_home.path().join("missing-identity-token"),
            )
            .env_remove("XDCODE_QUEUE_SERVER_API_KEY")
            .kill_on_drop(true)
            .args(["queue", "--resume", "--remote", "unix://"])
            .args(["--thread", &thread_id, "--message", message])
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let completed = timeout(Duration::from_secs(30), async {
        while let Some(event) = app.next_event().await {
            if let AppServerEvent::ServerNotification(notification) = event
                && let ServerNotification::TurnCompleted(completed) = *notification
            {
                return Ok::<_, anyhow::Error>(completed);
            }
        }
        anyhow::bail!("app server disconnected before the queued turn completed")
    })
    .await??;
    assert_eq!(completed.thread_id, thread_id);
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    let queue: ThreadQueueListResponse = app
        .request_typed(ClientRequest::ThreadQueueList {
            request_id: RequestId::Integer(2),
            params: ThreadQueueListParams {
                thread_id: thread_id.clone(),
                cursor: None,
                limit: None,
            },
        })
        .await?;
    assert!(queue.data.is_empty());

    let requests = model_requests.received_requests().await;
    let request = requests
        .first()
        .context("queued message did not reach the model")?;
    assert!(
        request.body_json::<Value>()?["input"]
            .to_string()
            .contains(message)
    );
    assert_eq!(
        request.headers["authorization"].to_str()?,
        "Bearer server-only-test-key"
    );
    let metadata: Value =
        serde_json::from_str(request.headers["x-xdcode-turn-metadata"].to_str()?)?;
    assert_eq!(metadata["turn_trigger"], "queue");

    let name = "remote-session-lifecycle";
    let _: ThreadSetNameResponse = app
        .request_typed(ClientRequest::ThreadSetName {
            request_id: RequestId::Integer(3),
            params: ThreadSetNameParams {
                thread_id: thread_id.clone(),
                name: name.to_string(),
            },
        })
        .await?;
    // A separate caller home makes name resolution rely on the selected server.
    let caller_home = TempDir::new()?;
    configure_queue_home(caller_home.path())?;
    let remote_command = || {
        let mut command = Command::new(&xdcode);
        command
            .env("XDCODE_HOME", caller_home.path())
            .current_dir(caller_home.path())
            .env(PROVIDER_FEDERATION_RULE_ID_ENV_VAR, "rule-test")
            .env(
                PROVIDER_IDENTITY_TOKEN_FILE_ENV_VAR,
                caller_home.path().join("missing-identity-token"),
            )
            .env_remove("XDCODE_QUEUE_SERVER_API_KEY")
            .kill_on_drop(true)
            .args(["--remote", &endpoint]);
        command
    };
    let output = timeout(
        Duration::from_secs(30),
        remote_command().args(["delete", &thread_id]).output(),
    )
    .await??;
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("cannot confirm session deletion"));

    let cold_message = "Continue after the thread was unloaded";
    let cases: &[(&[&str], Option<bool>)] = &[
        (&["archive", name], Some(true)),
        (&["unarchive", name], Some(false)),
        (&["delete", "--force", &thread_id], None),
    ];
    for &(args, expected_archived) in cases {
        let output = timeout(
            Duration::from_secs(30),
            remote_command().args(args).output(),
        )
        .await??;
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for archived in [false, true] {
            let threads: ThreadListResponse = app
                .request_typed(ClientRequest::ThreadList {
                    request_id: RequestId::Integer(4),
                    params: ThreadListParams {
                        excluded_thread_ids: None,
                        attached_to_thread_id: None,
                        cursor: None,
                        limit: None,
                        sort_key: None,
                        sort_direction: None,
                        model_providers: None,
                        source_kinds: None,
                        originators: None,
                        archived: Some(archived),
                        section_id: None,
                        project_id: None,
                        cwd: None,
                        use_state_db_only: false,
                        search_term: None,
                        parent_thread_id: None,
                        ancestor_thread_id: None,
                    },
                })
                .await?;
            let thread = threads.data.iter().find(|thread| thread.id == thread_id);
            assert_eq!(
                thread.is_some(),
                expected_archived == Some(archived),
                "{args:?}: archived={archived}"
            );
            if expected_archived == Some(false)
                && let Some(thread) = thread
            {
                assert_eq!(thread.status, ThreadStatus::NotLoaded);
            }
        }
        let Some(archived) = expected_archived else {
            continue;
        };
        // Archived threads are reachable by ID; active threads can also be targeted by name.
        let (target, text) = if archived {
            (thread_id.as_str(), "still archived")
        } else {
            (name, cold_message)
        };
        let output = remote_command()
            .args(["queue", "--resume", "--thread", target, "--message", text])
            .output()
            .await?;
        if archived {
            assert!(!output.status.success());
            assert!(
                String::from_utf8(output.stderr)?
                    .contains(&format!("session {thread_id} is archived"))
            );
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            timeout(
                Duration::from_secs(30),
                model_requests.wait_until_satisfied(),
            )
            .await?;
            let requests = model_requests.received_requests().await;
            assert!(
                requests[1].body_json::<Value>()?["input"]
                    .to_string()
                    .contains(cold_message)
            );
        }
    }
    Ok(())
}

#[test]
fn remote_session_commands_validate_config() -> Result<()> {
    let cases: &[(&[&str], &str, &str)] = &[
        (
            &["--strict-config"],
            "config.toml",
            "unknown_queue_option = true\n",
        ),
        (&["--profile", "queue"], "queue.config.toml", "model = 42\n"),
        (
            &[],
            "config.toml",
            "experimental_thread_store_endpoint = \"https://example.com\"\n",
        ),
    ];
    let commands: &[&[&str]] = &[
        &["queue", "--thread", THREAD_ID, "--message", "do the thing"],
        &["archive", THREAD_ID],
        &["unarchive", THREAD_ID],
        &["delete", "--force", THREAD_ID],
    ];
    for &command in commands {
        for &(args, config_file, config) in cases {
            let xdcode_home = TempDir::new()?;
            std::fs::write(xdcode_home.path().join(config_file), config)?;
            let output = std::process::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
                .env("XDCODE_HOME", xdcode_home.path())
                .current_dir(xdcode_home.path())
                .args(args)
                .args(["--remote", "ws://127.0.0.1:1"])
                .args(command)
                .output()?;
            let stderr = String::from_utf8(output.stderr)?;
            assert!(!output.status.success());
            assert!(
                stderr.contains("failed to load config.toml"),
                "{command:?}, {config:?}: {stderr}"
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn queue_rejects_local_daemon_that_does_not_support_queueing() -> Result<()> {
    let xdcode_home = socket_test_home()?;
    let socket_path = xdcode_app_server::app_server_control_socket_path(xdcode_home.path())?;
    std::fs::create_dir_all(
        socket_path
            .as_path()
            .parent()
            .context("missing socket parent")?,
    )?;
    let listener = tokio::net::UnixListener::bind(socket_path.as_path())?;
    let server_home = xdcode_home.path().to_path_buf();
    let server = tokio::spawn(async move {
        timeout(DEFAULT_TIMEOUT, async move {
            let (probe, _) = listener.accept().await?;
            drop(probe);
            let (stream, _) = listener.accept().await?;
            respond_to_queue_request(
                stream,
                server_home.as_path(),
                QueueResponse::UnknownMethodVariant,
            )
            .await
        })
        .await
        .context("local queue mock timed out")?
    });

    let output = timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", xdcode_home.path())
            .kill_on_drop(true)
            .args(["queue", "--thread", THREAD_ID, "--message", "do the thing"])
            .output(),
    )
    .await
    .context("local queue command timed out")??;
    server.await??;

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)?
            .contains("local app-server daemon does not support thread/queue/add")
    );
    assert!(!xdcode_home.path().join("queue_1.sqlite").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn queue_rejects_overrides_that_bypass_local_daemon() -> Result<()> {
    let xdcode_home = socket_test_home()?;
    let socket_path = xdcode_app_server::app_server_control_socket_path(xdcode_home.path())?;
    std::fs::create_dir_all(
        socket_path
            .as_path()
            .parent()
            .context("missing socket parent")?,
    )?;
    let listener = tokio::net::UnixListener::bind(socket_path.as_path())?;
    let server = tokio::spawn(async move {
        timeout(DEFAULT_TIMEOUT, async move {
            let (probe, _) = listener.accept().await?;
            drop(probe);
            Ok::<_, std::io::Error>(())
        })
        .await
        .context("local daemon probe timed out")?
        .context("local daemon probe failed")
    });

    let output = timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new(xdcode_utils_cargo_bin::cargo_bin("xdcode")?)
            .env("XDCODE_HOME", xdcode_home.path())
            .kill_on_drop(true)
            .args([
                "queue",
                "-C",
                ".",
                "-c",
                "analytics.enabled=false",
                "--thread",
                THREAD_ID,
                "--message",
                "do the thing",
            ])
            .output(),
    )
    .await
    .context("queue override rejection timed out")??;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("embedded app server while a local app-server daemon is running"),
        "{stderr}"
    );
    server.await??;
    Ok(())
}
