use std::sync::Arc;

use xdcode_api::AllowedCaller;
use xdcode_api::ApproximateLocation;
use xdcode_api::ExternalWebAccess;
use xdcode_api::ExternalWebAccessMode;
use xdcode_api::LocationType;
use xdcode_api::SearchContextSize;
use xdcode_api::SearchFilters;
use xdcode_api::SearchSettings;
use xdcode_core::config::Config;
use xdcode_extension_api::ConfigContributor;
use xdcode_extension_api::ExtensionData;
use xdcode_extension_api::ExtensionFuture;
use xdcode_extension_api::ExtensionRegistryBuilder;
use xdcode_extension_api::ThreadLifecycleContributor;
use xdcode_extension_api::ThreadOriginator;
use xdcode_extension_api::ThreadStartInput;
use xdcode_extension_api::ToolContributor;
use xdcode_http_client::HttpClientFactory;
use xdcode_login::AuthManager;
use xdcode_model_provider::WorkspaceRoutingContext;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_protocol::config_types::WebSearchContextSize;
use xdcode_protocol::config_types::WebSearchMode;
use xdcode_protocol::model_types::ModelInfo;

use crate::schema::LITERATURE_SEARCH_QUERY;
use crate::tool::WebSearchTool;

#[derive(Clone)]
struct WebSearchExtension {
    auth_manager: Arc<AuthManager>,
}

#[derive(Clone)]
struct WebSearchExtensionConfig {
    provider_supports_search: bool,
    http_client_factory: HttpClientFactory,
    provider: ModelProviderInfo,
    settings: SearchSettings,
}

impl From<&Config> for WebSearchExtensionConfig {
    fn from(config: &Config) -> Self {
        Self {
            provider_supports_search: config.model_provider.is_hosted_provider()
                || config.model_provider.uses_provider_actor_authorization()
                || config.model_provider.supports_standalone_web_search,
            http_client_factory: config.http_client_factory(),
            provider: config.model_provider.clone(),
            settings: search_settings(config),
        }
    }
}

fn search_settings(config: &Config) -> SearchSettings {
    let web_search_config = config.web_search_config.as_ref();
    SearchSettings {
        user_location: web_search_config
            .and_then(|config| config.user_location.as_ref())
            .map(|location| ApproximateLocation {
                r#type: LocationType::Approximate,
                country: location.country.clone(),
                region: location.region.clone(),
                city: location.city.clone(),
                timezone: location.timezone.clone(),
            }),
        search_context_size: web_search_config
            .and_then(|config| config.search_context_size)
            .map(|size| match size {
                WebSearchContextSize::Low => SearchContextSize::Low,
                WebSearchContextSize::Medium => SearchContextSize::Medium,
                WebSearchContextSize::High => SearchContextSize::High,
            }),
        filters: web_search_config
            .and_then(|config| config.filters.as_ref())
            .map(|filters| SearchFilters {
                allowed_domains: filters.allowed_domains.clone(),
                blocked_domains: None,
            }),
        allowed_callers: Some(vec![AllowedCaller::Direct]),
        ..Default::default()
    }
}

fn external_web_access_for_mode(web_search_mode: WebSearchMode) -> ExternalWebAccess {
    match web_search_mode {
        WebSearchMode::Disabled | WebSearchMode::Cached => ExternalWebAccess::Boolean(false),
        WebSearchMode::Indexed => ExternalWebAccess::Mode(ExternalWebAccessMode::Indexed),
        WebSearchMode::Live => ExternalWebAccess::Boolean(true),
    }
}

impl ThreadLifecycleContributor<Config> for WebSearchExtension {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(WebSearchExtensionConfig::from(input.config));
        })
    }
}

impl ConfigContributor<Config> for WebSearchExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(WebSearchExtensionConfig::from(new_config));
    }
}

impl ToolContributor for WebSearchExtension {
    fn tools_for_step(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
        step_store: &ExtensionData,
    ) -> Vec<
        Arc<
            dyn for<'call> xdcode_extension_api::ToolExecutor<xdcode_extension_api::ToolCall<'call>>,
        >,
    > {
        let (Some(config), Some(workspace_routing), Some(mode)) = (
            thread_store.get::<WebSearchExtensionConfig>(),
            thread_store.get::<WorkspaceRoutingContext>(),
            step_store.get::<WebSearchMode>(),
        ) else {
            return Vec::new();
        };
        if !config.provider_supports_search || *mode == WebSearchMode::Disabled {
            return Vec::new();
        }
        let auth = thread_store
            .get::<xdcode_model_provider::ProviderAuthContext>()
            .unwrap_or_else(|| {
                Arc::new(xdcode_model_provider::ProviderAuthContext {
                    credentials: Arc::clone(&self.auth_manager),
                    runtime_auth: None,
                })
            });

        vec![Arc::new(WebSearchTool {
            session_id: session_store.level_id().to_string(),
            http_client_factory: config.http_client_factory.clone(),
            provider: auth.model_provider(config.provider.clone()),
            workspace_routing,
            settings: SearchSettings {
                external_web_access: Some(external_web_access_for_mode(*mode)),
                ..config.settings.clone()
            },
            supports_literature_search: step_store.get::<ModelInfo>().is_some_and(|model| {
                model
                    .experimental_supported_tools
                    .iter()
                    .any(|tool| tool == LITERATURE_SEARCH_QUERY)
            }),
            originator: thread_store
                .get::<ThreadOriginator>()
                .map(|originator| originator.0.clone()),
        })]
    }
}

pub fn install(registry: &mut ExtensionRegistryBuilder<Config>, auth_manager: Arc<AuthManager>) {
    let extension = Arc::new(WebSearchExtension { auth_manager });
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.tool_contributor(extension);
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod routing_tests;

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use xdcode_extension_api::ExtensionData;
    use xdcode_extension_api::ExtensionRegistryBuilder;
    use xdcode_extension_api::ToolName;
    use xdcode_http_client::HttpClientFactory;
    use xdcode_http_client::OutboundProxyPolicy;
    use xdcode_login::XdcodeAuth;
    use xdcode_model_provider::WorkspaceRoutingContext;
    use xdcode_model_provider_info::ModelProviderInfo;

    use super::AuthManager;
    use super::Config;
    use super::WebSearchExtensionConfig;
    use super::external_web_access_for_mode;
    use super::install;
    use crate::tool::RUN_TOOL_NAME;
    use crate::tool::WEB_NAMESPACE;
    use xdcode_api::ExternalWebAccess;
    use xdcode_api::ExternalWebAccessMode;
    use xdcode_protocol::config_types::WebSearchMode;

    #[test]
    fn external_web_access_preserves_legacy_values_until_indexed() {
        assert_eq!(
            [
                WebSearchMode::Disabled,
                WebSearchMode::Cached,
                WebSearchMode::Indexed,
                WebSearchMode::Live,
            ]
            .map(external_web_access_for_mode),
            [
                ExternalWebAccess::Boolean(false),
                ExternalWebAccess::Boolean(false),
                ExternalWebAccess::Mode(ExternalWebAccessMode::Indexed),
                ExternalWebAccess::Boolean(true),
            ]
        );
    }

    #[test]
    fn installed_extension_contributes_web_run_when_enabled() {
        let mut builder = ExtensionRegistryBuilder::<Config>::new();
        install(
            &mut builder,
            AuthManager::from_auth_for_testing(XdcodeAuth::from_api_key("dummy")),
        );
        let registry = builder.build();
        let session_store = ExtensionData::new("session");
        let thread_store = ExtensionData::new("11111111-1111-4111-8111-111111111111");
        let step_store = ExtensionData::new("step");
        thread_store.insert(WorkspaceRoutingContext::new(
            "https://chatgpt.com/backend-api".to_string(),
        ));
        step_store.insert(WebSearchMode::Cached);
        thread_store.insert(WebSearchExtensionConfig {
            provider_supports_search: true,
            http_client_factory: HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            provider: ModelProviderInfo::create_custom_provider(/*base_url*/ None),
            settings: Default::default(),
        });

        let tool_names = registry
            .tool_contributors()
            .iter()
            .flat_map(|contributor| {
                contributor.tools_for_step(&session_store, &thread_store, &step_store)
            })
            .map(|tool| (tool.tool_name(), tool.supports_parallel_tool_calls()))
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names,
            vec![(ToolName::namespaced(WEB_NAMESPACE, RUN_TOOL_NAME), true)]
        );
    }
}
