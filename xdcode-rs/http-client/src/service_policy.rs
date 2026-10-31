//! Product-level egress policy, applied before DNS and again on redirects.
//! User providers and MCP servers on other hosts remain available.

use reqwest::Url;

use crate::NetworkPolicyDenied;

pub fn is_official_service_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    [
        "openai.com",
        "openai.org",
        "chatgpt.com",
        "chatgpt-staging.com",
        "oaistatic.com",
        "oaiusercontent.com",
        "ab.chatgpt.com",
        "o33249.ingest.us.sentry.io",
    ]
    .iter()
    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// This guard is independent of managed policy and cannot be opted out of.
/// It deliberately returns no URL, query string, or credentials in its error.
pub fn ensure_custom_service_url(url: &Url) -> Result<(), NetworkPolicyDenied> {
    if url.host_str().is_some_and(is_official_service_host) {
        Err(NetworkPolicyDenied::Destination)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_hosts_are_denied_including_subdomains_and_trailing_dots() {
        for host in [
            "api.openai.com",
            "AUTH.OPENAI.COM.",
            "chatgpt.com",
            "ab.chatgpt.com",
            "auth.api.openai.org",
            "chatgpt-staging.com",
            "o33249.ingest.us.sentry.io",
        ] {
            assert!(is_official_service_host(host), "{host}");
        }
    }

    #[test]
    fn custom_providers_and_mcp_hosts_are_allowed() {
        for host in [
            "models.example.invalid",
            "localhost",
            "127.0.0.1",
            "::1",
            "my-mcp.example",
            "provider.azure.com",
            "openai.com.example",
            "myopenai.com",
            "sentry.example",
        ] {
            assert!(!is_official_service_host(host), "{host}");
        }
    }

    #[tokio::test]
    async fn direct_and_routed_requests_fail_before_network_io() {
        let clients = [
            crate::HttpClientBuilder::new().build_direct().unwrap(),
            crate::RouteAwareClientPool::new(
                crate::HttpClientFactory::new(crate::OutboundProxyPolicy::ReqwestDefault),
                crate::ClientRouteClass::Other,
            )
            .into_client(),
        ];
        for client in clients {
            let error = client
                .get("https://api.openai.com/v1/models")
                .send()
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                crate::HttpError::Policy(NetworkPolicyDenied::Destination)
            ));
        }
    }
}
