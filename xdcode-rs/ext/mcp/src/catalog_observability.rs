//! Measures published cloud and V2 executor plugin catalogs, excluding cached reads.

use xdcode_core_plugins::PluginCatalog;
use xdcode_extension_api::ExtensionMetrics;
use xdcode_extension_api::record_catalog_entries;

pub(crate) fn record_catalog(
    metrics: Option<&dyn ExtensionMetrics>,
    stage: &'static str,
    catalog: &PluginCatalog,
) {
    record_catalog_entries(
        metrics,
        "xdcode.plugins.catalog.entries",
        stage,
        catalog.entries.len(),
        catalog.is_partial,
        catalog.warnings.len(),
    );
}
