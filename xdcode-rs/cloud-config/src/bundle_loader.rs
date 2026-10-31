use crate::config_server::config_server_http_client_factory;
use crate::config_source::BundleSource;
use crate::service::CloudConfigBundleService;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::AbortHandle;
use tokio::task::JoinHandle;
use xdcode_config::CloudConfigBundleLoader;
use xdcode_config::ConfigRequirementsToml;
use xdcode_http_client::HttpClientFactory;
use xdcode_login::AuthConfig;
use xdcode_login::AuthManager;

pub fn managed_config_auth_manager(
    auth: AuthConfig,
    requirements: &ConfigRequirementsToml,
) -> std::io::Result<Option<xdcode_login::OAuthSessionManager>> {
    let Some(service_config) = requirements.config_server.as_ref() else {
        return Ok(None);
    };
    let factory = config_server_http_client_factory(service_config, &auth.auth_route_config);
    crate::config_server_auth_manager(service_config, auth.xdcode_home, &factory)
}

pub fn managed_config_bundle_loader(
    auth_manager: Arc<AuthManager>,
    chatgpt_base_url: String,
    xdcode_home: PathBuf,
    factory: HttpClientFactory,
    requirements: &ConfigRequirementsToml,
) -> CloudConfigBundleLoader {
    if let Some(service_config) = requirements.config_server.as_ref() {
        let factory = config_server_http_client_factory(
            service_config,
            &xdcode_login::AuthRouteConfig::from_http_client_factory(factory),
        );
        return crate::config_server_bundle_loader(service_config.clone(), xdcode_home, factory);
    }
    let endpoint = xdcode_backend_client::Client::new(chatgpt_base_url.clone(), factory.clone())
        .config_bundle_url();
    let policy = factory
        .network_policy()
        .clone()
        .restrict_to_endpoints(endpoint.parse().into_iter().collect());
    cloud_config_bundle_loader(
        auth_manager,
        chatgpt_base_url,
        xdcode_home,
        factory.with_network_policy(policy),
    )
}

pub async fn managed_config_bundle_loader_for_storage(
    auth_config: AuthConfig,
    requirements: &ConfigRequirementsToml,
    enable_xdcode_api_key_env: bool,
) -> std::io::Result<CloudConfigBundleLoader> {
    if let Some(service_config) = requirements.config_server.as_ref() {
        let factory =
            config_server_http_client_factory(service_config, &auth_config.auth_route_config);
        return Ok(crate::config_server_bundle_loader(
            service_config.clone(),
            auth_config.xdcode_home,
            factory,
        ));
    }
    cloud_config_bundle_loader_for_storage(auth_config, enable_xdcode_api_key_env).await
}

struct CloudConfigBundleLoaderLifetime<C: BundleSource> {
    service: Arc<CloudConfigBundleService<C>>,
    refresh_task: AbortHandle,
}

impl<C: BundleSource> Drop for CloudConfigBundleLoaderLifetime<C> {
    fn drop(&mut self) {
        self.refresh_task.abort();
    }
}

pub fn cloud_config_bundle_loader(
    _auth_manager: Arc<AuthManager>,
    _chatgpt_base_url: String,
    _xdcode_home: PathBuf,
    _http_client_factory: HttpClientFactory,
) -> CloudConfigBundleLoader {
    CloudConfigBundleLoader::default()
}

pub(crate) fn cloud_config_bundle_loader_for_service<C>(
    service: CloudConfigBundleService<C>,
) -> (CloudConfigBundleLoader, JoinHandle<()>)
where
    C: BundleSource + 'static,
{
    let service = Arc::new(service);
    let background_service = Arc::clone(&service);
    let refresh_task = tokio::spawn(async move {
        let _ = background_service.get_latest().await;
        background_service.refresh_cache_in_background().await;
    });
    let abort_handle = refresh_task.abort_handle();
    let lifetime = Arc::new(CloudConfigBundleLoaderLifetime {
        service,
        refresh_task: abort_handle,
    });

    let snapshot_lifetime = Arc::clone(&lifetime);
    let policy = lifetime.service.policy.clone();
    let loader = CloudConfigBundleLoader::from_getter(move || {
        let lifetime = Arc::clone(&lifetime);
        async move { lifetime.service.get_latest().await }
    })
    .with_ema_policy_snapshots(policy, move || {
        let lifetime = Arc::clone(&snapshot_lifetime);
        async move { lifetime.service.get_latest_snapshot().await }
    });
    (loader, refresh_task)
}

pub async fn cloud_config_bundle_loader_for_storage(
    _auth_config: AuthConfig,
    _enable_xdcode_api_key_env: bool,
) -> std::io::Result<CloudConfigBundleLoader> {
    Ok(CloudConfigBundleLoader::default())
}

/// Returns an empty local loader; hosted configuration is unavailable in xdcode.
pub async fn cloud_config_bundle_loader_for_storage_without_cache(
    _auth_config: AuthConfig,
    _enable_xdcode_api_key_env: bool,
) -> std::io::Result<CloudConfigBundleLoader> {
    Ok(CloudConfigBundleLoader::default())
}
