use anyhow::Result;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_xdcode::test_xdcode;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use test_case::test_case;
use xdcode_config::model_catalog::CatalogModel;
use xdcode_config::model_catalog::ModelCompatibility;
use xdcode_core::test_support::custom_provider;
use xdcode_features::Feature;
use xdcode_protocol::config_types::ModeKind;
use xdcode_protocol::model_types::ReasoningEffort;
use xdcode_protocol::protocol::CatalogModelSelection;
use xdcode_protocol::protocol::EventMsg;
use xdcode_protocol::protocol::ThreadSettingsOverrides;

fn body(request: &wiremock::Request) -> Value {
    let bytes = if request
        .headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        == Some("zstd")
    {
        zstd::stream::decode_all(std::io::Cursor::new(&request.body)).unwrap()
    } else {
        request.body.clone()
    };
    serde_json::from_slice(&bytes).unwrap()
}

#[test_case(false, false, false; "wire_model_keeps_provider")]
#[test_case(true, false, false; "catalog_id_changes_provider")]
#[test_case(false, true, false; "wire_model_after_parent_catalog_switch")]
#[test_case(true, true, false; "catalog_id_after_parent_catalog_switch")]
#[test_case(true, true, true; "service_default_after_parent_catalog_switch")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_catalog_model_routes_the_actual_child_request(
    cross_provider: bool,
    parent_catalog_switch: bool,
    service_default: bool,
) -> Result<()> {
    let parent_server = start_mock_server().await;
    let other_server = start_mock_server().await;
    let child_server = if cross_provider {
        &other_server
    } else {
        &parent_server
    };
    let mut args = json!({
        "message": "Return the worker result.",
        "model": if cross_provider { "worker-b" } else { "gpt-6.1-sol" },
    });
    if !service_default {
        args["reasoning_effort"] = json!("balanced");
    }
    let args = args.to_string();
    mount_sse_once_match(
        &parent_server,
        |request: &wiremock::Request| {
            let body = body(request);
            body["model"] == "gpt-6-astra"
                && !body["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["type"] == "function_call_output")
        },
        sse(vec![
            ev_response_created("parent-spawn"),
            ev_function_call_with_namespace("spawn-worker", "multi_agent_v1", "spawn_agent", &args),
            ev_completed("parent-spawn"),
        ]),
    )
    .await;
    let parent_done = mount_sse_once_match(
        &parent_server,
        |request: &wiremock::Request| {
            let body = body(request);
            body["model"] == "gpt-6-astra"
                && body["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["type"] == "function_call_output")
        },
        sse(vec![
            ev_assistant_message("parent-done", "Delegated."),
            ev_completed("parent-done"),
        ]),
    )
    .await;
    let child_request = mount_sse_once_match(
        child_server,
        |request: &wiremock::Request| body(request)["model"] == "gpt-6.1-sol",
        sse(vec![
            ev_assistant_message("child-done", "Worker result."),
            ev_completed("child-done"),
        ]),
    )
    .await;
    let other_url = format!("{}/v1", other_server.uri());
    let test = test_xdcode()
        .with_config(move |config| {
            // This fixture keeps its catalog in memory; selection must not reload
            // the unrelated minimal config file created by the mock-server helper.
            config.config_layer_stack = xdcode_config::ConfigLayerStack::default();
            config.features.enable(Feature::Collab).unwrap();
            config.features.disable(Feature::MultiAgentV2).unwrap();
            config.model_provider_id = "a".into();
            config
                .model_providers
                .insert("a".into(), config.model_provider.clone());
            let mut other_provider = custom_provider(other_url.clone());
            other_provider.experimental_bearer_token = Some("worker-b-test-credential".into());
            config.model_providers.insert("b".into(), other_provider);
            config.models = [
                ("parent", "a", "gpt-6-astra"),
                ("worker-a", "a", "gpt-6.1-sol"),
                ("worker-b", "b", "gpt-6.1-sol"),
            ]
            .into_iter()
            .map(|(id, provider, model)| CatalogModel {
                id: id.into(),
                name: id.into(),
                provider: provider.into(),
                model: model.into(),
                reasoning_efforts: Some(if service_default && id != "parent" {
                    Vec::new()
                } else {
                    vec![
                        ReasoningEffort::High,
                        ReasoningEffort::Custom("balanced".into()),
                    ]
                }),
                default_reasoning_effort: (!(service_default && id != "parent"))
                    .then_some(ReasoningEffort::High),
                compatibility: ModelCompatibility::Responses,
                ..Default::default()
            })
            .collect();
            config.model = Some("gpt-6-astra".into());
            config.model_catalog_id = Some("parent".into());
            config.model_reasoning_effort = Some(ReasoningEffort::High);
        })
        .build_with_auto_env(&parent_server)
        .await?;
    if parent_catalog_switch {
        // Like an acknowledged /model operation, this installs the parent's pinned
        // runtime client. The child must not reuse its endpoint or thread identity.
        test.xdcode
            .update_thread_settings(ThreadSettingsOverrides {
                catalog_selection: Some(CatalogModelSelection {
                    provider: "a".into(),
                    catalog_id: "parent".into(),
                    model: "gpt-6-astra".into(),
                    effort: Some(ReasoningEffort::High),
                    mode: ModeKind::Default,
                }),
                ..Default::default()
            })
            .await?;
    }
    test.submit_text_turn("Delegate this task to the configured worker.")
        .await?;
    assert_eq!(parent_done.requests().len(), 1);
    let (spawn_output, _) = parent_done.requests()[0]
        .function_call_output_content_and_success("spawn-worker")
        .expect("spawn output");
    let spawn_output: Value = serde_json::from_str(&spawn_output.expect("spawn output text"))?;
    assert_eq!(
        spawn_output["selection"],
        json!({
            "catalog_id": if cross_provider { "worker-b" } else { "worker-a" },
            "model": "gpt-6.1-sol",
            "provider": if cross_provider { "b" } else { "a" },
            "reasoning_effort": (!service_default).then_some("balanced"),
        })
    );
    let child_id = test
        .thread_manager
        .list_thread_ids()
        .await
        .into_iter()
        .find(|id| *id != test.session_configured.session_id.into())
        .ok_or_else(|| anyhow::anyhow!("spawn must create a child"))?;
    let child = test.thread_manager.get_thread(child_id).await?;
    wait_for_event(&child, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    let requests = child_request.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body_json()["model"], "gpt-6.1-sol");
    if service_default {
        assert!(
            requests[0]
                .body_json()
                .pointer("/reasoning/effort")
                .is_none()
        );
    } else {
        assert_eq!(requests[0].body_json()["reasoning"]["effort"], "balanced");
    }
    assert_eq!(
        requests[0].header("authorization").as_deref(),
        Some(if cross_provider {
            "Bearer worker-b-test-credential"
        } else {
            "Bearer xdcode-test-credential"
        })
    );
    let child_config = child.config_snapshot().await;
    assert_eq!(child_config.model, "gpt-6.1-sol");
    assert_eq!(
        child_config.reasoning_effort,
        (!service_default).then(|| ReasoningEffort::Custom("balanced".into()))
    );
    assert_eq!(
        child_config.model_provider_id,
        if cross_provider { "b" } else { "a" }
    );
    let parent_config = test.xdcode.config_snapshot().await;
    assert_eq!(parent_config.model, "gpt-6-astra");
    assert_eq!(parent_config.model_provider_id, "a");
    assert_eq!(parent_config.reasoning_effort, Some(ReasoningEffort::High));
    child.shutdown_and_wait().await?;
    test.xdcode.shutdown_and_wait().await?;
    Ok(())
}
