use super::agent;
use core_test_support::responses::start_mock_server;
use core_test_support::test_xdcode::test_xdcode;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tempfile::TempDir;
use xdcode_config::Constrained;
use xdcode_core::config::Permissions;
use xdcode_model_provider::create_model_provider;
use xdcode_protocol::models::ManagedFileSystemPermissions;
use xdcode_protocol::models::PermissionProfile;
use xdcode_protocol::permissions::FileSystemPolicyProjection;
use xdcode_protocol::permissions::FileSystemSandboxPolicy;
use xdcode_protocol::permissions::NetworkSandboxPolicy;
use xdcode_protocol::protocol::AskForApproval;

#[tokio::test]
async fn consolidation_uses_canonical_parent_enforcement() -> anyhow::Result<()> {
    let server = start_mock_server().await;
    let home = Arc::new(TempDir::new()?);
    let test = test_xdcode()
        .with_home(home)
        .build_with_auto_env(&server)
        .await?;
    let provider = create_model_provider(
        test.config.model_provider.clone(),
        Some(test.thread_manager.auth_manager()),
    );

    let root = crate::memory_root(&test.config.xdcode_home);
    let managed_worker_profile = PermissionProfile::from_runtime_permissions(
        &FileSystemSandboxPolicy::workspace_write(
            std::slice::from_ref(&root),
            /*exclude_tmpdir_env_var*/ true,
            /*exclude_slash_tmp*/ true,
        ),
        NetworkSandboxPolicy::Restricted,
    );

    for (parent_permission_profile, expected_permission_profile) in [
        (PermissionProfile::Disabled, PermissionProfile::Disabled),
        (
            PermissionProfile::External {
                network: NetworkSandboxPolicy::Restricted,
            },
            PermissionProfile::External {
                network: NetworkSandboxPolicy::Restricted,
            },
        ),
        (
            PermissionProfile::External {
                network: NetworkSandboxPolicy::Enabled,
            },
            PermissionProfile::External {
                network: NetworkSandboxPolicy::Enabled,
            },
        ),
        (
            PermissionProfile::Managed {
                file_system: ManagedFileSystemPermissions::Unrestricted,
                network: NetworkSandboxPolicy::Enabled,
            },
            managed_worker_profile,
        ),
    ] {
        let agent_config =
            agent::get_config(&test.config, parent_permission_profile, provider.as_ref())
                .expect("agent config should be created");

        assert_eq!(
            (
                agent_config.permissions.permission_profile().enforcement(),
                agent_config.permissions.network_sandbox_policy(),
                agent_config.permissions.approval_policy.value(),
            ),
            (
                expected_permission_profile.enforcement(),
                expected_permission_profile.network_sandbox_policy(),
                AskForApproval::Never,
            ),
        );
        assert_eq!(
            agent_config
                .permissions
                .effective_permission_profile()
                .file_system_sandbox_policy()
                .resolve_for_enforcement(root.as_path(), FileSystemPolicyProjection::Native)?,
            expected_permission_profile
                .file_system_sandbox_policy()
                .resolve_for_enforcement(root.as_path(), FileSystemPolicyProjection::Native)?,
        );
    }

    let mut constrained_config = test.config.clone();
    constrained_config.permissions = Permissions::from_approval_and_profile(
        Constrained::allow_only(AskForApproval::Never),
        Constrained::allow_only(PermissionProfile::read_only()),
    )?;
    assert!(
        agent::get_config(
            &constrained_config,
            PermissionProfile::workspace_write(),
            provider.as_ref(),
        )
        .is_none()
    );

    test.xdcode.shutdown_and_wait().await?;
    Ok(())
}
