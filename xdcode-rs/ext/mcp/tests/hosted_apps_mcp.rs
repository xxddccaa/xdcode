use std::sync::Arc;

use pretty_assertions::assert_eq;
use xdcode_config::McpServerTransportConfig;
use xdcode_config::test_support::CloudConfigBundleFixture;
use xdcode_core::McpManager;
use xdcode_core::config::Config;
use xdcode_core::config::ConfigBuilder;
use xdcode_core::plugins_manager_for_config;
use xdcode_extension_api::ExtensionRegistryBuilder;
use xdcode_extension_api::McpServerContribution;
use xdcode_extension_api::McpServerContributionContext;
use xdcode_extension_api::McpServerContributor;
use xdcode_login::AuthManager;
use xdcode_login::XdcodeAuth;
use xdcode_login::test_support::auth_manager_from_optional_auth;
use xdcode_mcp::XDCODE_APPS_MCP_SERVER_NAME;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn contributes_hosted_plugin_runtime_without_an_executor() -> TestResult {
    let xdcode_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .xdcode_home(xdcode_home.path().to_path_buf())
        .fallback_cwd(Some(xdcode_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            ("chatgpt_base_url".to_string(), "https://chatgpt.com".into()),
        ])
        .build()
        .await?;
    let auth = XdcodeAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(XDCODE_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should be contributed as a configured server")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("hosted plugin runtime should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn runtime_overlay_preserves_disabled_server() -> TestResult {
    let xdcode_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .xdcode_home(xdcode_home.path().to_path_buf())
        .fallback_cwd(Some(xdcode_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.xdcode_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
            ("mcp_servers.xdcode_apps.enabled".to_string(), false.into()),
        ])
        .cloud_config_bundle(
            CloudConfigBundleFixture::loader_with_enterprise_requirement("[mcp_servers]"),
        )
        .build()
        .await?;
    let auth = XdcodeAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = installed_manager(&config, Some(auth.clone()));

    let configured = manager.configured_servers(&config).await;
    // Standalone login must still reject requirements-denied, explicitly disabled entries.
    let login_client = Arc::new(xdcode_exec_server::RouteAwareHttpClient::new(
        config.http_client_factory(),
    ));
    assert_eq!(
        xdcode_mcp::apply_http_headers_helper(
            login_client,
            &configured[XDCODE_APPS_MCP_SERVER_NAME],
            xdcode_home.path().to_path_buf(),
        )
        .err(),
        Some("the MCP server is disabled by managed requirements".to_string())
    );
    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(XDCODE_APPS_MCP_SERVER_NAME)
        .ok_or("hosted plugin runtime should remain configured")?;

    assert!(!server.enabled());
    Ok(())
}

#[tokio::test]
async fn default_fallback_overwrites_reserved_config_without_an_extension() -> TestResult {
    let xdcode_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .xdcode_home(xdcode_home.path().to_path_buf())
        .fallback_cwd(Some(xdcode_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), true.into()),
            (
                "mcp_servers.xdcode_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let auth = XdcodeAuth::create_dummy_chatgpt_auth_for_testing();
    let manager = McpManager::new(Arc::new(plugins_manager_for_config(
        &config,
        AuthManager::from_auth_for_testing(auth.clone()),
    )));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    let server = servers
        .get(XDCODE_APPS_MCP_SERVER_NAME)
        .ok_or("default Apps MCP should be present")?
        .config();
    let McpServerTransportConfig::StreamableHttp { url, .. } = &server.transport else {
        panic!("default Apps MCP should use streamable HTTP");
    };
    assert_eq!(url, "https://chatgpt.com/backend-api/ps/mcp");

    Ok(())
}

#[tokio::test]
async fn later_extension_can_remove_same_name_registration() -> TestResult {
    let xdcode_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .xdcode_home(xdcode_home.path().to_path_buf())
        .fallback_cwd(Some(xdcode_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = XdcodeAuth::create_dummy_chatgpt_auth_for_testing();
    let mut builder = ExtensionRegistryBuilder::new();
    xdcode_mcp_extension::install(&mut builder);
    builder.mcp_server_contributor(Arc::new(RemoveXdcodeApps));
    let manager = McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            &config,
            AuthManager::from_auth_for_testing(auth.clone()),
        )),
        Arc::new(builder.build()),
        xdcode_core::XdcodeAppsToolsCache::default(),
    );

    let servers = manager.effective_servers(&config, Some(&auth)).await;

    assert!(!servers.contains_key(XDCODE_APPS_MCP_SERVER_NAME));
    Ok(())
}

#[tokio::test]
async fn hosted_apps_mcp_requires_chatgpt_auth() -> TestResult {
    let xdcode_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .xdcode_home(xdcode_home.path().to_path_buf())
        .fallback_cwd(Some(xdcode_home.path().to_path_buf()))
        .cli_overrides(vec![("features.apps".to_string(), true.into())])
        .build()
        .await?;
    let auth = XdcodeAuth::from_api_key("test");
    let manager = installed_manager(&config, Some(auth.clone()));

    let servers = manager.effective_servers(&config, Some(&auth)).await;
    assert!(!servers.contains_key(XDCODE_APPS_MCP_SERVER_NAME));

    Ok(())
}

#[tokio::test]
async fn disabled_apps_remove_reserved_server_config_for_all_hosts() -> TestResult {
    let xdcode_home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .xdcode_home(xdcode_home.path().to_path_buf())
        .fallback_cwd(Some(xdcode_home.path().to_path_buf()))
        .cli_overrides(vec![
            ("features.apps".to_string(), false.into()),
            (
                "mcp_servers.xdcode_apps.url".to_string(),
                "https://example.com/mcp".into(),
            ),
        ])
        .build()
        .await?;
    let managers = [
        installed_manager(&config, /*auth*/ None),
        McpManager::new(Arc::new(plugins_manager_for_config(
            &config,
            auth_manager_from_optional_auth(/*auth*/ None),
        ))),
    ];
    for manager in managers {
        let servers = manager.runtime_servers(&config).await;
        assert!(!servers.contains_key(XDCODE_APPS_MCP_SERVER_NAME));
    }
    Ok(())
}

fn installed_manager(config: &Config, auth: Option<XdcodeAuth>) -> McpManager {
    let mut builder = ExtensionRegistryBuilder::new();
    xdcode_mcp_extension::install(&mut builder);
    McpManager::new_with_extensions(
        Arc::new(plugins_manager_for_config(
            config,
            auth_manager_from_optional_auth(auth),
        )),
        Arc::new(builder.build()),
        xdcode_core::XdcodeAppsToolsCache::default(),
    )
}

struct RemoveXdcodeApps;

impl McpServerContributor<Config> for RemoveXdcodeApps {
    fn id(&self) -> &'static str {
        "remove_xdcode_apps"
    }

    fn contribute<'a>(
        &'a self,
        _context: McpServerContributionContext<'a, Config>,
    ) -> xdcode_extension_api::ExtensionFuture<'a, Vec<McpServerContribution>> {
        Box::pin(async move {
            vec![McpServerContribution::Remove {
                name: XDCODE_APPS_MCP_SERVER_NAME.to_string(),
            }]
        })
    }
}
