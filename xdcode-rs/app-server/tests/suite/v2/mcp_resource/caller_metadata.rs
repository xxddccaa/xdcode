//! Public JSON-RPC coverage for caller metadata on MCP resource reads.

use super::*;
use core_test_support::stdio_server_bin;
use pretty_assertions::assert_eq;
use xdcode_app_server_protocol::JSONRPCMessage;
use xdcode_app_server_protocol::JSONRPCRequest;
use xdcode_protocol::protocol::W3cTraceContext;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_resource_read_forwards_rpc_trace_context() -> Result<()> {
    let responses = responses::start_mock_server().await;
    let (url, calls, server) = start_resource_apps_mcp_server().await?;
    let (_home, mut mcp) = start_resource_test_app_server_with_extra_config(
        &url,
        &responses.uri(),
        "[otel]\ntrace_exporter = \"none\"",
        ResourceTestEnvironment::Auto,
    )
    .await?;
    let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let tracestate = "vendor=origin";
    let id = RequestId::String("traced-read".to_string());
    mcp.send_jsonrpc_message(JSONRPCMessage::Request(JSONRPCRequest {
        id: id.clone(),
        method: "mcpServer/resource/read".to_string(),
        params: Some(json!({"server": "xdcode_apps", "uri": TEST_RESOURCE_URI})),
        trace: Some(W3cTraceContext {
            traceparent: Some(traceparent.to_string()),
            tracestate: Some(tracestate.to_string()),
        }),
    }))
    .await?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(id),
    )
    .await??;
    server.abort();

    let reads = calls.read_metadata.lock().expect("resource read metadata");
    let read = reads.last().expect("resource read reached the MCP server");
    assert_eq!(read.get("traceparent"), Some(&json!(traceparent)));
    assert_eq!(read.get("tracestate"), Some(&json!(tracestate)));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_resource_receives_caller_metadata() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let xdcode_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .with_extra_config(&format!(
            "[mcp_servers.custom]\ncommand = {}",
            toml::Value::String(stdio_server_bin()?),
        ))
        .write(xdcode_home.path())?;
    let mut mcp = TestAppServer::builder()
        .with_xdcode_home(xdcode_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;
    let ThreadStartResponse { thread, .. } = mcp
        .request(|request_id| ClientRequest::ThreadStart {
            request_id,
            params: ThreadStartParams::default(),
        })
        .await?;
    let mut params = json!({
        "threadId": thread.id,
        "server": "custom",
        "uri": "test://xdcode/request-metadata",
    });
    let metadata = json!({
        "com.openai/xdcode-statusline-state": {
            "config": {"statusline_items": ["model", "context-remaining"]},
            "thread": {"id": thread.id, "name": null},
        },
        "connector_id": "custom-connector",
        "link_id": "custom-link",
        "_xdcode_apps": {"resource_uri": "custom://route"},
    });
    for caller_metadata in [None, Some(&metadata)] {
        if let Some(meta) = caller_metadata {
            params["_meta"] = meta.clone();
        }
        let request = mcp
            .send_raw_request("mcpServer/resource/read", Some(params.clone()))
            .await?;
        let response: McpResourceReadResponse =
            timeout(DEFAULT_READ_TIMEOUT, mcp.read_response(request)).await??;
        let [McpResourceContent::Text { text, .. }] = response.contents.as_slice() else {
            anyhow::bail!("expected the metadata echoed by the stdio server");
        };
        let received: serde_json::Value = serde_json::from_str(text)?;
        for key in metadata.as_object().unwrap().keys() {
            assert_eq!(
                received.get(key),
                caller_metadata.and_then(|meta| meta.get(key))
            );
        }
    }
    for (invalid_meta, expected_message) in [
        (json!({"threadId": "spoofed"}), "threadId"),
        (
            json!({"x-xdcode-turn-metadata": {}}),
            "x-xdcode-turn-metadata",
        ),
        (
            json!({"codex/connectionLease": "spoofed"}),
            "codex/connectionLease",
        ),
        (json!([]), "map"),
    ] {
        params["_meta"] = invalid_meta;
        let request = mcp
            .send_raw_request("mcpServer/resource/read", Some(params.clone()))
            .await?;
        let error = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_error_message(RequestId::Integer(request)),
        )
        .await??;
        assert_eq!(error.error.code, -32600);
        assert!(
            error.error.message.contains(expected_message),
            "{}",
            error.error.message
        );
    }
    Ok(())
}
