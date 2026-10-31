//! Local curated-snapshot lookup. xdcode never synchronizes upstream plugins.

use std::path::Path;
use std::path::PathBuf;

use xdcode_http_client::HttpClientFactory;

// Retained solely to recognize and reject the upstream curated source in policy.
pub(crate) const PROVIDER_PLUGINS_GIT_URL: &str = "https://github.com/openai/plugins.git";

pub fn curated_plugins_repo_path(xdcode_home: &Path) -> PathBuf {
    xdcode_home.join(".tmp/plugins")
}

pub fn curated_plugins_api_marketplace_path(xdcode_home: &Path) -> PathBuf {
    curated_plugins_repo_path(xdcode_home).join(".agents/plugins/api_marketplace.json")
}

pub fn read_curated_plugins_sha(xdcode_home: &Path) -> Option<String> {
    std::fs::read_to_string(xdcode_home.join(".tmp/plugins.sha"))
        .ok()
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| !sha.is_empty())
}

pub fn has_local_curated_plugins_snapshot(xdcode_home: &Path) -> bool {
    curated_plugins_repo_path(xdcode_home)
        .join(".agents/plugins/marketplace.json")
        .is_file()
        && xdcode_home.join(".tmp/plugins.sha").is_file()
}

pub fn sync_provider_plugins_repo(
    _xdcode_home: &Path,
    _http_client_factory: HttpClientFactory,
) -> Result<String, String> {
    Err("Official curated plugins are unavailable in xdcode; configure a local or custom marketplace".to_owned())
}

#[cfg(test)]
#[path = "startup_sync_tests.rs"]
mod tests;
