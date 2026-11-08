use std::fs;

use pretty_assertions::assert_eq;
use tempfile::TempDir;
use xdcode_exec_server::CapabilityRootDiscoverRequest;
use xdcode_exec_server::CapabilityRootsDiscoverParams;
use xdcode_exec_server::FileSystemEnvironmentAccessor;
use xdcode_exec_server::LOCAL_FS;
use xdcode_exec_server::discover_capability_roots;
use xdcode_protocol::protocol::Product;
use xdcode_protocol::protocol::SkillScope;
use xdcode_skills::SkillDependencies;
use xdcode_skills::SkillPolicy;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_path_uri::PathUri;
use xdcode_utils_plugins::PluginIdentity;
use xdcode_utils_plugins::PluginSkillRoot;
use xdcode_utils_plugins::SkillDiscoveryMode;

use super::HostSkillRoot;
use super::SKILLS_METADATA_FILENAME;
use super::SKILLS_PORTABLE_METADATA_FILENAME;
use super::load_environment_skills_from_discovery;
use super::load_environment_skills_from_root;
use super::load_host_skill_root;

fn fixture(native: bool) -> TempDir {
    assert_ne!(SKILLS_METADATA_FILENAME, SKILLS_PORTABLE_METADATA_FILENAME);
    let root = TempDir::new().expect("temporary skills root");
    let skill = root.path().join("demo");
    fs::create_dir_all(skill.join("agents")).expect("metadata directory");
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: demo\ndescription: Imported skill.\n---\n\nInstructions.\n",
    )
    .expect("skill instructions");
    fs::write(
        skill.join("agents").join(SKILLS_PORTABLE_METADATA_FILENAME),
        document("portable-server", false),
    )
    .expect("portable input metadata");
    if native {
        fs::write(
            skill.join("agents").join(SKILLS_METADATA_FILENAME),
            document("native-server", true),
        )
        .expect("native metadata");
    }
    root
}

fn document(server: &str, implicit: bool) -> String {
    let product = if implicit { "xdcode" } else { "codex" };
    format!(
        "dependencies:\n  tools:\n    - type: mcp\n      value: {server}\n      transport: stdio\n      command: local-tool\npolicy:\n  allow_implicit_invocation: {implicit}\n  products: [{product}]\n"
    )
}

fn assert_metadata(
    dependencies: Option<&SkillDependencies>,
    policy: Option<&SkillPolicy>,
    native: bool,
) {
    let tools = &dependencies.expect("dependencies preserved").tools;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].r#type, "mcp");
    assert_eq!(
        tools[0].value,
        if native {
            "native-server"
        } else {
            "portable-server"
        }
    );
    assert_eq!(tools[0].command.as_deref(), Some("local-tool"));
    assert_eq!(
        policy.expect("policy preserved").allow_implicit_invocation,
        Some(native)
    );
    assert_eq!(policy.unwrap().products, vec![Product::Xdcode]);
}

#[tokio::test]
async fn host_scan_preserves_portable_policy_and_native_priority() {
    let access = FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS);
    for native in [false, true] {
        let root = fixture(native);
        let snapshot = load_host_skill_root(HostSkillRoot::host(
            AbsolutePathBuf::from_absolute_path(root.path()).expect("root path"),
            SkillScope::User,
            &access,
        ))
        .await;
        assert!(snapshot.errors.is_empty());
        assert_eq!(snapshot.skills.len(), 1);
        assert_metadata(
            snapshot.skills[0].dependencies.as_ref(),
            snapshot.skills[0].policy.as_ref(),
            native,
        );

        // Direct-child inventories do not include the metadata files themselves.
        let plugin_root = AbsolutePathBuf::from_absolute_path(root.path()).expect("plugin root");
        let snapshot = load_host_skill_root(HostSkillRoot::plugin(
            PluginSkillRoot {
                path: plugin_root.clone(),
                plugin_identity: PluginIdentity {
                    plugin_id: "test@local".into(),
                    remote_plugin_id: None,
                },
                plugin_namespace: "test".into(),
                plugin_root,
                discovery_mode: SkillDiscoveryMode::DirectChildren,
            },
            &access,
        ))
        .await;
        assert!(snapshot.errors.is_empty());
        assert_eq!(snapshot.skills.len(), 1);
        assert_metadata(
            snapshot.skills[0].dependencies.as_ref(),
            snapshot.skills[0].policy.as_ref(),
            native,
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn canonical_host_remap_keeps_portable_metadata() {
    let access = FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS);
    for native in [false, true] {
        let target = fixture(native);
        let root = TempDir::new().expect("alias root");
        std::os::unix::fs::symlink(target.path().join("demo"), root.path().join("alias"))
            .expect("skill directory alias");
        let snapshot = load_host_skill_root(HostSkillRoot::host(
            AbsolutePathBuf::from_absolute_path(root.path()).expect("alias root path"),
            SkillScope::User,
            &access,
        ))
        .await;
        assert!(snapshot.errors.is_empty());
        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(
            snapshot.skills[0].path_to_skills_md,
            PathUri::from_host_native_path(
                fs::canonicalize(target.path().join("demo/SKILL.md")).expect("canonical skill")
            )
            .expect("canonical URI")
        );
        assert_metadata(
            snapshot.skills[0].dependencies.as_ref(),
            snapshot.skills[0].policy.as_ref(),
            native,
        );
    }
}

#[tokio::test]
async fn bundled_and_direct_environment_metadata_match() {
    for native in [false, true] {
        let root = fixture(native);
        let root_uri = PathUri::from_host_native_path(root.path()).expect("root URI");
        let direct = load_environment_skills_from_root(
            &FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS),
            &root_uri,
            Some(Product::Xdcode),
        )
        .await;
        let bundle = discover_capability_roots(
            LOCAL_FS.as_ref(),
            CapabilityRootsDiscoverParams {
                roots: vec![CapabilityRootDiscoverRequest {
                    id: "test".into(),
                    path: root_uri,
                    sandbox: None,
                }],
            },
        )
        .await
        .expect("capability bundle");
        let discovered = &bundle.roots[0];
        assert!(discovered.error.is_none());
        assert!(discovered.warnings.is_empty());
        let bundled = load_environment_skills_from_discovery(discovered, Some(Product::Xdcode));
        assert_eq!(bundled.warnings, direct.warnings);
        assert_eq!(direct.skills.len(), 1);
        assert_eq!(bundled.skills.len(), 1);
        assert_eq!(bundled.skills[0].metadata, direct.skills[0]);
        assert_metadata(
            direct.skills[0].dependencies.as_ref(),
            direct.skills[0].policy.as_ref(),
            native,
        );
        assert_eq!(
            discovered.skills[0]
                .metadata
                .as_ref()
                .expect("metadata file")
                .path
                .basename()
                .as_deref(),
            Some(if native {
                SKILLS_METADATA_FILENAME
            } else {
                SKILLS_PORTABLE_METADATA_FILENAME
            })
        );
    }
}

#[tokio::test]
async fn invalid_native_metadata_keeps_existing_error_behavior() {
    let access = FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS);
    let root = fixture(true);
    fs::write(
        root.path()
            .join("demo/agents")
            .join(SKILLS_METADATA_FILENAME),
        "policy: [invalid]",
    )
    .expect("invalid native metadata");
    let root_uri = PathUri::from_host_native_path(root.path()).expect("root URI");
    let direct = load_environment_skills_from_root(
        &FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS),
        &root_uri,
        None,
    )
    .await;
    assert_eq!(direct.skills.len(), 1);
    assert!(direct.skills[0].policy.is_none());
    assert!(direct.skills[0].dependencies.is_none());
    let host = load_host_skill_root(HostSkillRoot::host(
        AbsolutePathBuf::from_absolute_path(root.path()).expect("root path"),
        SkillScope::User,
        &access,
    ))
    .await;
    assert_eq!(host.skills.len(), 1);
    assert!(host.skills[0].policy.is_none());
    assert!(host.skills[0].dependencies.is_none());
}
