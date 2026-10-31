use crate::ModelsManagerConfig;
use crate::manager::ModelsManager;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use xdcode_protocol::model_types::TruncationPolicyConfig;

use super::TestModelsEndpoint;
use super::provider_manager_for_tests;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_without_tool_output_override() {
    let xdcode_home = TempDir::new().expect("create temp dir");
    let config = ModelsManagerConfig::default();
    let manager = provider_manager_for_tests(
        xdcode_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("unknown-model", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::bytes(/*limit*/ 10_000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_with_tool_output_override() {
    let xdcode_home = TempDir::new().expect("create temp dir");
    let config = ModelsManagerConfig {
        tool_output_token_limit: Some(123),
        ..Default::default()
    };
    let manager = provider_manager_for_tests(
        xdcode_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("gpt-5.5", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::tokens(/*limit*/ 123)
    );
}
