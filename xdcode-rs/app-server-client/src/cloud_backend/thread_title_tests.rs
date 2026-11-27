#![cfg(test)]

use super::CloudTitleClient;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use xdcode_http_client::HttpClientFactory;
use xdcode_http_client::OutboundProxyPolicy;
use xdcode_model_provider::BearerAuthProvider;
use xdcode_protocol::model_types::ReasoningEffort;

#[tokio::test]
async fn generates_a_structured_title_using_the_selected_cloud_account() -> color_eyre::Result<()> {
    let server = MockServer::start().await;
    // Inject an already validated bootstrap so the test can use a loopback server.
    let client = CloudTitleClient {
        factory: HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        auth: Arc::new(BearerAuthProvider::for_test(
            Some("token"),
            Some("cloud-account"),
        )),
        account_id: "cloud-account".to_owned(),
        bootstrap: format!("{}/backend-api", server.uri()).parse()?,
        require_origin_match: false,
    };
    Mock::given(method("GET"))
        .and(path("/backend-api/wham/accounts/check"))
        .and(header("authorization", "Bearer token"))
        .and(header("chatgpt-account-id", "cloud-account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accounts": [
            {"id": "other-account", "workspace_backend_origin": "https://other.chatgpt.com", "account_routing_override": "us"},
            {"id": "cloud-account", "workspace_backend_origin": "NO_CONSTRAINT", "account_routing_override": "us_cr"}
        ]})))
        .expect(1)
        .mount(&server)
        .await;

    let schema = json!({
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"],
        "additionalProperties": false,
    });
    let title = r#"{"title":"Login timeout"}"#;
    let sse = [
        json!({"type": "response.output_item.done", "item": {
            "type": "message", "role": "assistant", "phase": "commentary",
            "content": [{"type": "output_text", "text": "Considering a title"}]
        }}),
        json!({"type": "response.output_item.done", "item": {
            "type": "message", "role": "assistant", "phase": "final_answer",
            "content": [{"type": "output_text", "text": title}]
        }}),
        json!({"type": "response.completed", "response": {"id": "response-1"}}),
    ]
    .into_iter()
    .map(|event| format!("data: {event}\n\n"))
    .collect::<String>();
    Mock::given(method("POST"))
        .and(path("/backend-api/xdcode/responses"))
        .and(header("authorization", "Bearer token"))
        .and(header("chatgpt-account-id", "cloud-account"))
        .and(header("x-openai-account-routing-override", "us_cr"))
        .and(body_json(json!({
            "model": "title-model",
            "instructions": "Generate a thread title from the user prompt.",
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Fix the login timeout"}]}],
            "store": false,
            "stream": true,
            "tools": [],
            "tool_choice": "none",
            "parallel_tool_calls": false,
            "include": [],
            "client_metadata": {"x-xdcode-turn-metadata": r#"{"thread_source":"thread_title"}"#},
            "text": {"format": {"type": "json_schema", "strict": true, "schema": schema, "name": "xdcode_output_schema"}},
            "reasoning": {"effort": "low"},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        client
            .generate(
                "title-model",
                Some(ReasoningEffort::Low),
                "Fix the login timeout".to_owned(),
                schema,
            )
            .await?,
        title
    );
    Ok(())
}
