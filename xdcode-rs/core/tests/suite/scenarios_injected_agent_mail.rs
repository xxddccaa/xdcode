//! Pending agent messages must survive interrupted sampling and compaction.

use super::*;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[test_case::test_case(true; "instant interrupt enabled")]
#[test_case::test_case(false; "instant interrupt disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn injected_agent_message_interrupting_sampling_survives_mid_turn_compaction(
    instant_interrupt: bool,
) -> anyhow::Result<()> {
    let (release_boundary, boundary_gate) = oneshot::channel();
    let chunk = |gate, events| StreamingSseChunk {
        gate,
        body: responses::sse(events),
    };
    let long_reasoning = "reasoning ".repeat(400);
    let (server, _completions) = start_streaming_sse_server(vec![
        vec![
            chunk(
                None,
                vec![
                    responses::ev_response_created("first"),
                    responses::ev_reasoning_item_added("boundary", &[long_reasoning.as_str()]),
                ],
            ),
            chunk(
                Some(boundary_gate),
                vec![responses::ev_reasoning_item(
                    "boundary",
                    &[long_reasoning.as_str()],
                    &[],
                )],
            ),
        ],
        vec![chunk(
            None,
            vec![
                responses::ev_response_created("compact"),
                responses::ev_assistant_message("summary", "AUTO_COMPACT_SUMMARY"),
                responses::ev_completed_with_tokens("compact", 50),
            ],
        )],
        vec![chunk(
            None,
            vec![
                responses::ev_response_created("after-compact"),
                responses::ev_assistant_message("answer", "The worker update is included."),
                responses::ev_completed_with_tokens("after-compact", 60),
            ],
        )],
    ])
    .await;
    let config_server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    let test = test_xdcode()
        .with_model("gpt-5.4")
        .with_config(move |config| {
            config.model_provider.name = "Provider (test)".to_string();
            config.model_provider.base_url = Some(base_url);
            config.model_provider.supports_websockets = false;
            config.model_auto_compact_token_limit = Some(200);
            config
                .features
                .set_enabled(Feature::InstantInterrupt, instant_interrupt)
                .expect("set instant interrupt");
            config
                .features
                .disable(Feature::DeferMailboxPreemption)
                .expect("enable preemption");
            config
                .features
                .disable(Feature::EnableRequestCompression)
                .expect("capture request bodies");
        })
        .build_with_auto_env(&config_server)
        .await?;
    let message: ResponseItem = serde_json::from_value(json!({
        "type": "agent_message", "id": "agent_active-before-compaction",
        "author": "/root/worker", "recipient": "/root",
        "content": [{"type": "input_text", "text": "Use the completed worker result."}],
    }))?;
    test.xdcode
        .start_or_steer_turn(TurnInputRequest::user_input(vec![super::text(
            "Plan the next action.",
        )]))
        .await?;
    wait_for_event(
        &test.xdcode,
        |event| matches!(event, EventMsg::ItemStarted(item) if item.item.id() == "boundary"),
    )
    .await;
    test.xdcode
        .inject_response_items(vec![message.clone()])
        .await?;
    release_boundary
        .send(())
        .expect("release sampling boundary");
    wait_for_event(&test.xdcode, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    test.xdcode.shutdown_and_wait().await?;

    let requests = server
        .requests()
        .await
        .iter()
        .map(|body| serde_json::from_slice::<Value>(body))
        .collect::<serde_json::Result<Vec<_>>>()?;
    assert_eq!(requests.len(), 3);
    let after_compact_input = requests[2]["input"].as_array().expect("request input");
    let actual = after_compact_input
        .iter()
        .filter(|item| item["type"] == "agent_message")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        responses::strip_metadata_from_json(json!(actual)),
        responses::strip_metadata_from_json(serde_json::to_value([message])?)
    );
    let entries = requests.iter().map(SnapshotEntry::body).collect::<Vec<_>>();
    insta::assert_snapshot!("injected_agent_message_interrupting_sampling_survives_mid_turn_compaction", context_snapshot::format_context_snapshot(
        "An injected agent message that interrupts sampling is included in the first request after mid-turn compaction.",
        &entries,
        &ContextSnapshotOptions::default().rewrite_known_segments(),
    ).replace(test.home.path().to_str().unwrap(), "<XDCODE_HOME>"));
    server.shutdown().await;
    Ok(())
}
