//! Two real agents share the detached board through ordinary HTTP MCP configuration.

use super::*;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_board_is_shared_with_children() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let home = Arc::new(tempfile::tempdir()?);
    let sqlite = xdcode_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into()?);
    let _state = xdcode_state::StateRuntime::init(sqlite.clone(), "openai".into()).await?;
    let command = xdcode_utils_cargo_bin::cargo_bin("xdcode-message-board-mcp")?;
    let mut process =
        tokio::process::Command::from(xdcode_utils_process::background_command(command))
            .arg("--sqlite-home")
            .arg(sqlite.home())
            .args(["--listen", "127.0.0.1:0", "--allowed-host", "board.example"])
            .env("XDCODE_MESSAGE_BOARD_TOKEN", "http-board-test-token")
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
    let ready = tokio::io::BufReader::new(process.stdout.take().context("server stdout")?)
        .lines()
        .next_line()
        .await?
        .context("server readiness")?;
    let ready: Value = serde_json::from_str(&ready)?;
    let endpoint = json!({"url":ready["url"],"http_headers":{"Authorization":"Bearer http-board-test-token","Host":"board.example"},"required":true,"default_tools_approval_mode":"approve"});
    let configured_endpoint = endpoint.clone();
    let configured_sqlite = sqlite.clone();
    let configure_board = move |config: &mut xdcode_core::config::Config| {
        configure(config);
        config.sqlite = configured_sqlite.clone();
        config
            .features
            .disable(Feature::AgentMessageBoard)
            .expect("disable native board");
        let mut servers = config.mcp_servers.get().clone();
        servers.insert(
            "xdcode_message_board".into(),
            serde_json::from_value(configured_endpoint.clone()).expect("HTTP config"),
        );
        config.mcp_servers.set(servers).expect("configure endpoint");
    };
    let mut builder = test_xdcode()
        .with_home(home)
        .with_config(configure_board.clone());
    let root = builder.build_with_auto_env(&server).await?;
    responses::mount_sse_sequence(
        &server,
        vec![
            tool(
                "spawn-worker",
                "spawn_agent",
                json!({"task_name":"worker","message":"Say ready.","fork_turns":"none"}),
            ),
            done(),
            done(),
        ],
    )
    .await;
    root.submit_turn("Spawn a worker and finish your turn.")
        .await?;
    let root_id = root.session_configured.thread_id;
    let child_id = root
        .thread_manager
        .list_thread_ids()
        .await
        .into_iter()
        .find(|id| *id != root_id)
        .expect("child runtime");
    let child = root.thread_manager.get_thread(child_id).await?;
    wait_for_event(&child, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    let mut host_state = None;
    if let Some(local) = &local {
        let host = Arc::new(board_host::Host {
            members: [
                (root_id, xdcode_protocol::AgentPath::root()),
                (
                    child_id,
                    xdcode_protocol::AgentPath::root()
                        .join("worker")
                        .expect("worker path"),
                ),
            ]
            .into(),
            clock: Default::default(),
            fail_clock: Default::default(),
            agent_path_calls: Default::default(),
            active: Default::default(),
            fail_notifications: Default::default(),
            notifications: Default::default(),
        });
        host_state = Some(Arc::downgrade(&host));
        local.add_board(root_id.into(), host)?;
    }
    let board_turn = |id: &str, name: &str, arguments: Value| {
        vec![
            sse(vec![
                responses::ev_tool_search_call("find-board", &json!({"query":name,"limit":1})),
                ev_completed("find-board"),
            ]),
            sse(vec![
                ev_function_call_with_namespace(
                    id,
                    "mcp__xdcode_message_board",
                    name,
                    &arguments.to_string(),
                ),
                ev_completed(id),
            ]),
            done(),
        ]
    };
    let board_result = |request: &ResponsesRequest, id: &str| -> anyhow::Result<Value> {
        let output = request
            .function_call_output_text(id)
            .with_context(|| format!("missing board result {id}"))?;
        let json = output
            .split_once("\nOutput:\n")
            .map_or(output.as_str(), |(_, json)| json);
        serde_json::from_str(json).with_context(|| format!("board result: {output}"))
    };
    let mut post_arguments =
        json!({"new_channel_name":"design","text":"Worker's durable decision."});
    if mode == BoardMode::Feature {
        post_arguments["agents_to_notify"] = json!(["/root"]);
    }
    let posted =
        responses::mount_sse_sequence(&server, board_turn("worker-post", "post", post_arguments))
            .await;
    child
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Find the board tools and post your decision.".into(),
            text_elements: vec![],
        }]))
        .await?;
    wait_for_event(&child, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    let post = board_result(
        posted.requests().last().context("child follow-up")?,
        "worker-post",
    )?;
    assert_eq!(post["author"], "/root/worker");
    if mode == BoardMode::InMemory {
        assert_eq!(post["created_at"], "2026-09-18T12:00:00Z");
    }
    if mode != BoardMode::Feature {
        child.shutdown_and_wait().await?;
    }

    let resumed = if in_memory {
        root
    } else {
        test_xdcode()
            .with_config(configure_board)
            .restart_with_auto_env(&server, &root)
            .await?
    };
    let writer = if in_memory {
        None
    } else {
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join("agent_message_board_1.sqlite"))
            .await?;
        Some(pool.begin_with("BEGIN IMMEDIATE").await?)
    };
    let read = responses::mount_sse_sequence(
        &server,
        board_turn(
            "root-read",
            "search_posts",
            json!({"channel_name":"design"}),
        ),
    )
    .await;
    resumed
        .submit_turn("Find the board tools and read the worker's decision.")
        .await?;
    if let Some(writer) = writer {
        writer.rollback().await?;
    }
    if mode == BoardMode::Feature {
        assert!(read.requests()[1].body_contains_text("Message Type: CHANNEL_POST"));
    }
    let result = board_result(
        read.requests().last().context("parent follow-up")?,
        "root-read",
    )?;
    assert_eq!(
        serde_json::from_value::<xdcode_message_board::Page<xdcode_message_board::PostPreview>>(
            result
        )?,
        xdcode_message_board::Page {
            results: vec![xdcode_message_board::PostPreview {
                metadata: serde_json::from_value(post.clone())?,
                text_preview: "Worker's durable decision.".into(),
                n_chars: "Worker's durable decision.".chars().count(),
                truncated: false,
            }],
            next_cursor: None,
        }
    );
    for request in [&posted.requests()[1], &read.requests()[1]] {
        assert!(
            request
                .tool_search_output("find-board")
                .to_string()
                .contains("mcp__xdcode_message_board")
        );
        assert!(request.tool_by_name("collaboration", "post").is_none());
    }
    let timestamp = post["created_at"].as_str().context("post timestamp")?;
    let mut bodies = posted
        .requests()
        .iter()
        .chain(read.requests().iter())
        .map(ResponsesRequest::body_json)
        .collect::<Vec<_>>();
    for body in &mut bodies {
        for item in body["input"].as_array_mut().context("request input")? {
            if item["type"] == "function_call_output"
                && let Some(output) = item["output"].as_str()
            {
                item["output"] = Value::String(
                    context_snapshot::normalize_json_lines(output)
                        .replace(timestamp, "<TIMESTAMP>"),
                );
            }
        }
    }
    insta::assert_snapshot!(
        "agent_message_board_mcp_context",
        context_snapshot::format_context_snapshot(
            "A child posts through the HTTP MCP executable; its restarted parent discovers the same board and reads the decision.",
            &bodies.iter().map(SnapshotEntry::body).collect::<Vec<_>>(),
            &ContextSnapshotOptions::default().rewrite_known_segments(),
        )
    );
    use xdcode_rmcp_client::RmcpClient;
    let url = endpoint["url"].as_str().context("endpoint URL")?;
    for (token, host) in [
        ("wrong-token", "board.example"),
        ("http-board-test-token", "unlisted.example"),
        ("http-board-test-token", "127.0.0.1"),
    ] {
        let client = RmcpClient::new_streamable_http_client(
            "board-auth-test",
            url,
            Some(token.to_owned()),
            Some([("Host".into(), host.into())].into()),
            None,
            xdcode_config::types::OAuthCredentialsStoreMode::File,
            Default::default(),
            xdcode_exec_server::Environment::default_for_tests().get_http_client(),
            None,
        )
        .await?;
        let initialized = client
            .initialize(
                rmcp::model::InitializeRequestParams::default()
                    .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_11_25),
                /*timeout*/ None,
                Box::new(|_, _| Box::pin(async { anyhow::bail!("unexpected elicitation") })),
            )
            .await;
        if token == "wrong-token" || host == "unlisted.example" {
            assert!(initialized.is_err());
            continue;
        }
        initialized?;
        for metadata in [
            None,
            Some(json!({"threadId":root_id, "sessionId":child_id})),
        ] {
            let error = client
                .call_tool(
                    "get_channels".into(),
                    None,
                    |_| Ok(metadata.clone()),
                    None,
                )
                .await
                .expect_err("invalid identity must fail");
            assert_eq!(
                xdcode_rmcp_client::mcp_error(&error).map(|error| error.code),
                Some(rmcp::model::ErrorCode::INVALID_PARAMS)
            );
        }
        connected_client = Some(client);
    }
    resumed.xdcode.shutdown_and_wait().await?;
    if let Some(local) = local {
        assert!(
            !resumed
                .config
                .sqlite_config()
                .home()
                .join("agent_message_board_1.sqlite")
                .exists()
        );
        local.shutdown().await?;
        assert!(host_state.context("in-memory host")?.upgrade().is_none());
    }
    if let Some(mut process) = process {
        process.kill().await?;
        process.wait().await?;
    }
    if let Some(client) = connected_client {
        client.shutdown().await;
    }
    Ok(())
}

#[tokio::test]
async fn http_shutdown_releases_pending_binding() -> anyhow::Result<()> {
    use xdcode_message_board::MessageBoardHost;
    use xdcode_rmcp_client::RmcpClient;
    use tokio_util::sync::CancellationToken;

    let caller = xdcode_protocol::ThreadId::new();
    let host = Arc::new(board_host::Host {
        members: [(caller, xdcode_protocol::AgentPath::root())].into(),
        clock: Default::default(),
        fail_clock: Default::default(),
        agent_path_calls: Default::default(),
        active: Default::default(),
        fail_notifications: Default::default(),
        notifications: Default::default(),
    });
    let host_state = Arc::downgrade(&host);
    let board = Arc::new(xdcode_message_board::InMemoryAgentMessageBoard::new(
        caller.into(),
        host.clone(),
    ));
    let binding_started = Arc::new(tokio::sync::Notify::new());
    let resume_binding = Arc::new(tokio::sync::Notify::new());
    let started = binding_started.clone();
    let resume = resume_binding.clone();
    let cancellation = CancellationToken::new();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let url = format!("http://{}/mcp", listener.local_addr()?);
    let serving = tokio::spawn(xdcode_message_board_mcp::serve_http(
        listener,
        "shutdown-test-token".into(),
        rmcp::transport::StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation.clone()),
        move |caller, _session_id| {
            let host = host.clone();
            let board = board.clone();
            let started = started.clone();
            let resume = resume.clone();
            async move {
                started.notify_one();
                resume.notified().await;
                let path = host.agent_path(caller).await.map_err(|error| {
                    rmcp::ErrorData::invalid_params(error.to_string(), None)
                })?;
                Ok(xdcode_message_board_mcp::MessageBoardMcpServer::new(
                    &xdcode_message_board::MessageBoardTool::DISCUSSION,
                    board,
                    caller,
                    path,
                ))
            }
        },
    ));
    let client = RmcpClient::new_streamable_http_client(
        "board-shutdown-test",
        &url,
        Some("shutdown-test-token".into()),
        None,
        None,
        xdcode_config::types::OAuthCredentialsStoreMode::File,
        Default::default(),
        xdcode_exec_server::Environment::default_for_tests().get_http_client(),
        None,
    )
    .await?;
    client
        .initialize(
            rmcp::model::InitializeRequestParams::default()
                .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_11_25),
            None,
            Box::new(|_, _| Box::pin(async { anyhow::bail!("unexpected elicitation") })),
        )
        .await?;
    {
        let call = client.call_tool(
            "post".into(),
            Some(json!({"new_channel_name":"design", "text":"Must not be dispatched."})),
            |_| Ok(Some(json!({"threadId":caller, "sessionId":caller}))),
            None,
        );
        tokio::pin!(call);
        tokio::select! {
            _ = binding_started.notified() => {}
            result = &mut call => panic!("call completed before binding resumed: {result:?}"),
        }
        cancellation.cancel();
        serving.await??;
        assert!(host_state.upgrade().is_none());
        resume_binding.notify_one();
    }
    client.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_shutdown_releases_factory_with_keep_alive_connection() -> anyhow::Result<()> {
    use xdcode_message_board::MessageBoardHost;
    use tokio::io::AsyncWriteExt;
    use tokio_util::sync::CancellationToken;

    for iteration in 0..256 {
        let caller = xdcode_protocol::ThreadId::new();
        let host = Arc::new(board_host::Host {
            members: [(caller, xdcode_protocol::AgentPath::root())].into(),
            clock: Default::default(),
            fail_clock: Default::default(),
            agent_path_calls: Default::default(),
            active: Default::default(),
            fail_notifications: Default::default(),
            notifications: Default::default(),
        });
        let host_state = Arc::downgrade(&host);
        let board = Arc::new(xdcode_message_board::InMemoryAgentMessageBoard::new(
            caller.into(),
            host.clone(),
        ));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let cancellation = CancellationToken::new();
        let shutdown = cancellation.clone();
        let connection = tokio::spawn(async move {
            let _shutdown = shutdown.drop_guard();
            let mut stream = tokio::net::TcpStream::connect(address).await?;
            stream
                .write_all(b"GET /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await?;
            let mut stream = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await?;
            assert_eq!(line, "HTTP/1.1 401 Unauthorized\r\n");
            while line != "\r\n" {
                line.clear();
                anyhow::ensure!(
                    stream.read_line(&mut line).await? != 0,
                    "incomplete response"
                );
            }
            Ok::<_, anyhow::Error>(stream)
        });
        xdcode_message_board_mcp::serve_http(
            listener,
            "keep-alive-test-token".into(),
            rmcp::transport::StreamableHttpServerConfig::default()
                .with_cancellation_token(cancellation),
            move |caller, _session_id| {
                let host = host.clone();
                let board = board.clone();
                async move {
                    let path = host.agent_path(caller).await.map_err(|error| {
                        rmcp::ErrorData::invalid_params(error.to_string(), None)
                    })?;
                    Ok(xdcode_message_board_mcp::MessageBoardMcpServer::new(
                        &xdcode_message_board::MessageBoardTool::DISCUSSION,
                        board,
                        caller,
                        path,
                    ))
                }
            },
        )
        .await?;
        assert!(
            host_state.upgrade().is_none(),
            "factory retained the board on iteration {iteration}",
        );
        drop(connection.await??);
    }
    Ok(())
}
