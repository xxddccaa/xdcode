use std::collections::HashMap;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn selects_only_supported_mcp_extensions() {
    let app_ui = json!({
        "mimeTypes": [
            "text/html;profile=mcp-app",
            "text/x-dil;profile=mcp-app",
        ],
        "futureField": {"preserved": true},
    });
    let form = json!({"futureField": {"preserved": true}});
    let extensions = HashMap::from([
        (
            PROVIDER_ELICITATION_EXTENSION_ID.to_string(),
            json!({"form": form, "userVerification": {}, "unsupported": {}}),
        ),
        (MCP_APP_UI_EXTENSION_ID.to_string(), app_ui.clone()),
        (PROVIDER_FORM_EXTENSION_ID.to_string(), json!({})),
        (
            PROVIDER_STANDARD_FORM_INPUT_EXTENSION_ID.to_string(),
            json!({}),
        ),
        ("example/other".to_string(), json!({"enabled": true})),
    ]);

    assert_eq!(
        client_mcp_extensions(
            Some(&extensions),
            /*legacy_provider_form_elicitation*/ false,
        ),
        ClientMcpExtensions::new(HashMap::from([
            (
                PROVIDER_ELICITATION_EXTENSION_ID.to_string(),
                json!({"form": form}),
            ),
            (MCP_APP_UI_EXTENSION_ID.to_string(), app_ui),
            (PROVIDER_FORM_EXTENSION_ID.to_string(), json!({})),
            (
                PROVIDER_STANDARD_FORM_INPUT_EXTENSION_ID.to_string(),
                json!({}),
            ),
        ]))
    );
}

#[test]
fn normalizes_legacy_form_capability_into_extensions() {
    assert_eq!(
        client_mcp_extensions(
            /*extensions*/ None, /*legacy_provider_form_elicitation*/ true,
        ),
        ClientMcpExtensions::new(HashMap::from([(
            PROVIDER_FORM_EXTENSION_ID.to_string(),
            json!({}),
        )]))
    );
}

#[test]
fn user_verification_is_projected_only_to_the_host_owned_plugin_service() {
    use crate::catalog::McpServerRegistration;
    use crate::catalog::ResolvedMcpCatalog;
    use crate::mcp::XDCODE_APPS_MCP_SERVER_NAME;
    use crate::mcp::xdcode_apps_mcp_server_config;
    use xdcode_rmcp_client::McpProtocolMode;

    let config = xdcode_apps_mcp_server_config(
        "https://example.com",
        /*apps_mcp_product_sku*/ None,
        /*originator*/ None,
    );
    let extensions = ClientMcpExtensions::new([
        (
            PROVIDER_ELICITATION_EXTENSION_ID.to_string(),
            json!({"form": {}, "userVerification": {}}),
        ),
        (
            crate::MCP_NETWORK_POLICY_META_CAPABILITY.to_string(),
            json!({}),
        ),
    ]);
    assert_eq!(
        server_mcp_extensions(
            &extensions,
            XDCODE_APPS_MCP_SERVER_NAME,
            /*server*/ None,
            /*advertise_network_policy*/ true,
            "https://chatgpt.com",
        ),
        ClientMcpExtensions::new([(
            PROVIDER_ELICITATION_EXTENSION_ID.to_string(),
            json!({"form": {}}),
        )]),
    );
    for (name, registration, settings) in [
        (
            XDCODE_APPS_MCP_SERVER_NAME,
            McpServerRegistration::from_hosted_apps(
                "host",
                /*contribution_order*/ 0,
                config.clone(),
            ),
            json!({"form": {}, "userVerification": {}}),
        ),
        (
            XDCODE_APPS_MCP_SERVER_NAME,
            McpServerRegistration::from_hosted_apps(
                "host",
                /*contribution_order*/ 0,
                config.clone(),
            )
            .with_protocol_mode(McpProtocolMode::V20260728),
            json!({"form": {}, "userVerification": {}}),
        ),
        (
            XDCODE_APPS_MCP_SERVER_NAME,
            McpServerRegistration::from_extension(
                XDCODE_APPS_MCP_SERVER_NAME.into(),
                "ordinary",
                /*contribution_order*/ 0,
                config.clone(),
            )
            .with_protocol_mode(McpProtocolMode::V20260728),
            json!({"form": {}}),
        ),
        (
            XDCODE_APPS_MCP_SERVER_NAME,
            McpServerRegistration::from_config(XDCODE_APPS_MCP_SERVER_NAME.into(), config.clone()),
            json!({"form": {}}),
        ),
        (
            "attached",
            McpServerRegistration::from_config("attached".into(), config),
            json!({"form": {}}),
        ),
    ] {
        let mut catalog = ResolvedMcpCatalog::builder();
        catalog.register(registration);
        let catalog = catalog.build();
        assert_eq!(
            server_mcp_extensions(
                &extensions,
                name,
                catalog.server(name),
                /*advertise_network_policy*/ false,
                "https://chatgpt.com",
            ),
            ClientMcpExtensions::new([(PROVIDER_ELICITATION_EXTENSION_ID.to_string(), settings)]),
        );
    }
}
