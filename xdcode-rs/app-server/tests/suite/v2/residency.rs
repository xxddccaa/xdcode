use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::write_models_cache;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::timeout;
use xdcode_app_server_protocol::ConfigWarningNotification;
use xdcode_app_server_protocol::ThreadStartParams;
use xdcode_app_server_protocol::TurnStartParams;
use xdcode_app_server_protocol::TurnStatus;
use xdcode_app_server_protocol::UserInput;
use xdcode_login::default_client::RESIDENCY_HEADER_NAME;

const READ_TIMEOUT: Duration = Duration::from_secs(60);
const PROVIDER_RESIDENCY_ENV_VAR: &str = "XDCODE_TEST_RESIDENCY_HEADER";

#[derive(Clone, Copy)]
enum ModelTransport {
    Http,
    Websocket,
}

async fn write_provider_config(
    xdcode_home: &Path,
    model_base_url: &str,
    transport: ModelTransport,
) -> Result<()> {
    let supports_websockets = matches!(transport, ModelTransport::Websocket);
    let config = format!(
        r#"model = "gpt-5.4"
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_provider = "custom-provider"

[features]
shell_snapshot = false

[model_providers.custom-provider]
name = "Provider"
base_url = "{model_base_url}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
requires_hosted_auth = true
supports_websockets = {supports_websockets}

[model_providers.custom-provider.http_headers]
"X-OpenAI-Internal-Xdcode-Residency" = "eu-static"
"x-provider-header" = "preserved"

[model_providers.custom-provider.env_http_headers]
"x-openai-internal-codex-residency" = "{PROVIDER_RESIDENCY_ENV_VAR}"
"#
    );
    std::fs::write(xdcode_home.join("config.toml"), config)?;
    write_models_cache(xdcode_home).await?;
    Ok(())
}

#[test_case(ModelTransport::Http; "http")]
#[test_case(ModelTransport::Websocket; "websocket")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_residency_overrides_provider_headers(transport: ModelTransport) -> Result<()> {
    skip_if_no_network!(Ok(()));

    let http_server = responses::start_mock_server().await;
    let http_responses = responses::mount_sse_once(
        &http_server,
        responses::sse(vec![
            responses::ev_response_created("http-response"),
            responses::ev_assistant_message("http-message", "done"),
            responses::ev_completed("http-response"),
        ]),
    )
    .await;
    let websocket_server = responses::start_websocket_server(vec![vec![
        vec![
            responses::ev_response_created("prewarm-response"),
            responses::ev_completed("prewarm-response"),
        ],
        vec![
            responses::ev_response_created("websocket-response"),
            responses::ev_assistant_message("websocket-message", "done"),
            responses::ev_completed("websocket-response"),
        ],
    ]])
    .await;
    let model_base_url = match transport {
        ModelTransport::Http => http_server.uri(),
        ModelTransport::Websocket => websocket_server.uri().to_string(),
    };

    let xdcode_home = TempDir::new()?;
    write_provider_config(xdcode_home.path(), &model_base_url, transport).await?;
    std::fs::write(
        xdcode_home.path().join("requirements.toml"),
        "enforce_residency = \"us\"\n",
    )?;

    let mut app_server = TestAppServer::builder()
        .with_xdcode_home(xdcode_home.path())
        .with_env_overrides(&[
            ("PROVIDER_API_KEY", Some("sk-test")),
            ("XDCODE_ACCESS_TOKEN", None),
            (PROVIDER_RESIDENCY_ENV_VAR, Some("eu-environment")),
        ])
        .build_initialized_with_timeout(READ_TIMEOUT)
        .await?;

    let expected_summary = format!(
        "Ignoring `{RESIDENCY_HEADER_NAME}` in `model_providers.custom-provider` because managed residency is required."
    );
    let notification = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_matching_notification(
            "managed residency configWarning",
            |notification| {
                notification.method == "configWarning"
                    && notification
                        .params
                        .as_ref()
                        .and_then(|params| params.get("summary"))
                        .and_then(Value::as_str)
                        == Some(expected_summary.as_str())
            },
        ),
    )
    .await??;
    let warning: ConfigWarningNotification = serde_json::from_value(
        notification
            .params
            .context("managed residency warning should have parameters")?,
    )?;
    assert_eq!(
        warning,
        ConfigWarningNotification {
            summary: expected_summary,
            details: None,
            path: None,
            range: None,
        }
    );

    let thread = app_server
        .start_thread(ThreadStartParams::default())
        .await?;
    let completed = timeout(
        READ_TIMEOUT,
        app_server.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "hello".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;
    assert_eq!(completed.turn.status, TurnStatus::Completed);

    let (residency_header, provider_header) = match transport {
        ModelTransport::Http => {
            let request = http_responses.single_request();
            (
                request.header(RESIDENCY_HEADER_NAME),
                request.header("x-provider-header"),
            )
        }
        ModelTransport::Websocket => {
            let handshake = websocket_server.single_handshake();
            (
                handshake.header(RESIDENCY_HEADER_NAME),
                handshake.header("x-provider-header"),
            )
        }
    };
    assert_eq!(residency_header, Some("us".to_string()));
    assert_eq!(provider_header, Some("preserved".to_string()));

    websocket_server.shutdown().await;
    Ok(())
}
