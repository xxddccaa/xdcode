use std::collections::HashMap;

use serde_json::Map;
use serde_json::Value;
use xdcode_protocol::mcp::ClientMcpExtensions;
use xdcode_protocol::mcp::MCP_APP_UI_EXTENSION_ID;
use xdcode_protocol::mcp::PROVIDER_ELICITATION_EXTENSION_ID;
use xdcode_protocol::mcp::PROVIDER_FORM_EXTENSION_ID;
use xdcode_protocol::mcp::PROVIDER_STANDARD_FORM_INPUT_EXTENSION_ID;

/// Projects privileged capabilities only to their intended recipients.
/// Verification requires the host-owned plugin service; network policy requires a trusted
/// ChatGPT origin. Server names and caller-supplied capabilities do not grant either privilege.
pub(crate) fn server_mcp_extensions(
    extensions: &ClientMcpExtensions,
    server_name: &str,
    server: Option<&crate::catalog::ResolvedMcpServer>,
    advertise_network_policy: bool,
    chatgpt_base_url: &str,
) -> ClientMcpExtensions {
    let plugin_service = server.is_some_and(|server| {
        server
            .source()
            .is_host_owned_apps(server_name, server.config())
    });
    let network_policy = advertise_network_policy
        && server.is_some_and(|server| {
            crate::mcp::is_trusted_chatgpt_mcp_server(&server.config().transport, chatgpt_base_url)
        });
    ClientMcpExtensions::new(
        extensions
            .iter()
            .filter(|(id, _)| *id != crate::MCP_NETWORK_POLICY_META_CAPABILITY)
            .map(|(id, settings)| {
                let mut settings = settings.clone();
                if !plugin_service
                    && id == PROVIDER_ELICITATION_EXTENSION_ID
                    && let Some(settings) = settings.as_object_mut()
                {
                    settings.remove("userVerification");
                }
                (id.to_string(), settings)
            })
            .chain(network_policy.then(|| {
                (
                    crate::MCP_NETWORK_POLICY_META_CAPABILITY.to_string(),
                    Value::Object(Map::new()),
                )
            })),
    )
}

/// Selects the MCP extensions Xdcode supports from those declared by the app-server host.
///
/// App-server clients may declare unrelated extensions. Xdcode retains only the
/// trusted extension namespaces it knows how to project downstream. The
/// legacy form capability is normalized into the same extension map.
pub fn client_mcp_extensions(
    extensions: Option<&HashMap<String, Value>>,
    legacy_provider_form_elicitation: bool,
) -> ClientMcpExtensions {
    let mut selected = extensions
        .into_iter()
        .flat_map(HashMap::iter)
        .filter(|(id, _)| {
            matches!(
                id.as_str(),
                PROVIDER_FORM_EXTENSION_ID
                    | PROVIDER_ELICITATION_EXTENSION_ID
                    | PROVIDER_STANDARD_FORM_INPUT_EXTENSION_ID
                    | MCP_APP_UI_EXTENSION_ID
            )
        })
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect::<HashMap<_, _>>();
    if let Some(settings) = selected
        .get_mut(PROVIDER_ELICITATION_EXTENSION_ID)
        .and_then(Value::as_object_mut)
    {
        settings.retain(|key, _| key == "form");
    }
    if legacy_provider_form_elicitation {
        selected
            .entry(PROVIDER_FORM_EXTENSION_ID.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    ClientMcpExtensions::new(selected)
}

#[cfg(test)]
#[path = "client_capabilities_tests.rs"]
mod tests;
