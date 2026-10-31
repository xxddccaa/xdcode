use super::*;
use xdcode_http_client::OutboundProxyPolicy;

#[test]
fn disabled_sync_does_not_create_runtime_state() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("not-created");
    let result = sync_provider_plugins_repo(
        &home,
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
    );
    assert!(result.is_err());
    assert!(!home.exists());
}
