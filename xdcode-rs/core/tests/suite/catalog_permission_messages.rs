use anyhow::Result;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_models_once;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_xdcode::test_xdcode;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use std::fs;
use test_case::test_case;
use wiremock::MockServer;
use xdcode_core::TurnInputRequest;
use xdcode_core::config::Constrained;
use xdcode_login::XdcodeAuth;
use xdcode_models_manager::manager::RefreshStrategy;
use xdcode_models_manager::model_info::model_info_from_slug;
use xdcode_protocol::model_types::ApprovalMessages;
use xdcode_protocol::model_types::ModelMessages;
use xdcode_protocol::model_types::ModelsResponse;
use xdcode_protocol::model_types::PermissionMessages;
use xdcode_protocol::models::PermissionProfile;
use xdcode_protocol::protocol::AskForApproval;
use xdcode_protocol::protocol::EventMsg;
use xdcode_protocol::protocol::ThreadSettingsOverrides;
use xdcode_protocol::user_input::UserInput;

#[test_case(Some("Catalog approval instructions."), "Catalog approved prefixes: {{ approved_prefixes }}", Some(r#"Catalog approved prefixes: - ["echo"]"#); "catalog_prefixes")]
#[test_case(None, "", None; "empty_catalog_prefixes")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_permission_message_loaded_from_remote_models_is_sent(
    catalog_approval: Option<&str>,
    prefix_template: &str,
    expected_prefix_line: Option<&str>,
) -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    let model_slug = "remote-catalog-permissions-model";
    let mut model = model_info_from_slug(model_slug);
    model.model_messages = Some(ModelMessages {
        content_filter_guidance: None,
        persistent_instructions: None,
        tools: None,
        instructions_template: None,
        approvals: catalog_approval.map(|on_request| ApprovalMessages {
            on_request: Some(on_request.to_string()),
            on_request_auto_review: None,
            never: None,
            unless_trusted: None,
        }),
        collaboration_modes: None,
        auto_review: None,
        permissions: Some(PermissionMessages {
            danger_full_access: None,
            workspace_write: None,
            read_only: Some("remote catalog permissions: {{ network_access }}".to_string()),
            approved_prefixes: Some(prefix_template.to_string()),
            ..PermissionMessages::default()
        }),
        multi_agent: None,
        token_budget: None,
        confirmation_policies: None,
        guardian_v2: None,
    });
    let models_mock = mount_models_once(
        &server,
        ModelsResponse {
            models: vec![model],
        },
    )
    .await;
    let response_mock = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-1"), ev_completed("resp-1")]),
    )
    .await;
    let mut builder = test_xdcode()
        .with_auth(XdcodeAuth::create_dummy_chatgpt_auth_for_testing())
        .with_config(|config| {
            config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
            let rules_dir = config.xdcode_home.join("rules");
            fs::create_dir_all(&rules_dir).expect("create rules directory");
            fs::write(
                rules_dir.join("approved.rules"),
                r#"prefix_rule(pattern=["echo"], decision="allow")"#,
            )
            .expect("write approved prefix");
            config
                .permissions
                .set_permission_profile(PermissionProfile::read_only())
                .expect("read-only permission profile should be allowed");
        });
    let test = builder.build_with_auto_env(&server).await?;
    test.thread_manager
        .get_models_manager()
        .list_models(
            RefreshStrategy::OnlineIfUncached,
            xdcode_core::test_support::default_http_client_factory(),
        )
        .await;

    core_test_support::submit_thread_settings(
        &test.xdcode,
        ThreadSettingsOverrides {
            model: Some(model_slug.to_string()),
            ..Default::default()
        },
    )
    .await?;
    test.xdcode
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(&test.xdcode, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    assert_eq!(models_mock.single_request_path(), "/v1/models");
    let permissions = response_mock
        .single_request()
        .message_input_texts("developer")
        .into_iter()
        .filter(|text| text.contains("<permissions instructions>"))
        .map(|text| text.replace("\r\n", "\n"))
        .collect::<Vec<_>>();
    let [permissions]: [String; 1] = permissions.try_into().expect("one permission fragment");
    assert!(
        permissions
            .starts_with("<permissions instructions>\nremote catalog permissions: restricted\n")
    );
    assert!(permissions.contains(catalog_approval.unwrap_or("prefix_rule")));
    assert_eq!(
        permissions
            .lines()
            .filter(|line| line.contains(r#"["echo"]"#))
            .collect::<Vec<_>>(),
        expected_prefix_line.into_iter().collect::<Vec<_>>()
    );
    Ok(())
}
