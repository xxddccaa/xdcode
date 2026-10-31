//! xdcode never discovers upstream releases, including in release builds.

use crate::legacy_core::config::Config;

pub fn get_upgrade_version(_config: &Config) -> Option<String> {
    None
}

pub fn get_upgrade_version_for_popup(_config: &Config) -> Option<String> {
    None
}
