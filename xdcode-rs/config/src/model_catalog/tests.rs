use super::*;
use tempfile::tempdir;

#[test]
fn configuration_transactions_share_the_model_cli_lock_and_release_after_errors() {
    let temporary = tempdir().expect("temporary config");
    let path = temporary.path().join("config.toml");
    fs::write(&path, "# retained\nmodel = 'before'\n").expect("config");
    let mut transaction = ConfigurationFile::open(&path).expect("first writer");
    assert!(ConfigurationFile::open(&path).is_err());
    transaction.document()["model"] = toml_edit::value("after");
    transaction.finish(false).expect("atomic update");
    let transaction = ConfigurationFile::open(&path).expect("lock released");
    assert!(transaction.check_version(Some("stale"), true).is_err());
    drop(transaction);
    assert!(ConfigurationFile::open(&path).is_ok());
    assert!(
        fs::read_to_string(&path)
            .expect("config")
            .contains("# retained")
    );
}

#[test]
fn configuration_transactions_preserve_unrelated_legacy_and_unknown_fields() {
    let temporary = tempdir().expect("temporary config");
    let path = temporary.path().join("config.toml");
    let original =
        "# retained\nprofiles = { fast = { sandbox_mode = 'strict' } }\nunknown_setting = 'keep'\n";
    fs::write(&path, original).expect("config");
    let mut transaction = ConfigurationFile::open(&path).expect("transaction");
    transaction.document()["model"] = toml_edit::value("changed");
    transaction.finish(false).expect("atomic update");
    let updated = fs::read_to_string(path).expect("updated config");
    assert!(updated.starts_with(original));
    assert!(updated.contains("model = \"changed\""));
}

#[test]
fn configuration_transactions_reject_invalid_catalog_fields_without_writing() {
    let temporary = tempdir().expect("temporary config");
    let path = temporary.path().join("config.toml");
    let original = "model = 'before'\n";
    fs::write(&path, original).expect("config");
    let mut transaction = ConfigurationFile::open(&path).expect("transaction");
    transaction.document()["models"] = toml_edit::value("invalid");
    assert!(transaction.finish(false).is_err());
    assert_eq!(
        fs::read_to_string(&path).expect("unchanged config"),
        original
    );
    assert!(ConfigurationFile::open(&path).is_ok());
}

#[cfg(unix)]
#[test]
fn configuration_transaction_rejects_a_retargeted_symlink_without_writing_either_file() {
    let temporary = tempdir().expect("temporary config");
    let first = temporary.path().join("first.toml");
    let second = temporary.path().join("second.toml");
    let path = temporary.path().join("config.toml");
    let original = "model = 'before'\n";
    fs::write(&first, original).expect("first config");
    fs::write(&second, original).expect("second config");
    std::os::unix::fs::symlink(&first, &path).expect("config symlink");
    let mut transaction = ConfigurationFile::open(&path).expect("transaction");
    transaction.document()["model"] = toml_edit::value("ours");
    fs::remove_file(&path).expect("unlink config");
    std::os::unix::fs::symlink(&second, &path).expect("new config target");
    assert!(transaction.finish(false).is_err());
    assert_eq!(fs::read_to_string(first).expect("first config"), original);
    assert_eq!(fs::read_to_string(second).expect("second config"), original);
}

#[test]
fn empty_and_commented_configurations_load_as_documents() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for raw in ["", "# user comment\n", "model = 'legacy-model'\n"] {
        fs::write(&path, raw).unwrap();
        let snapshot = CatalogSnapshot::load(&path).unwrap();
        assert!(snapshot.models.is_empty());
    }
}

fn provider() -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Test".into(),
        base_url: Some("http://127.0.0.1:43210/v1".into()),
        ..Default::default()
    }
}

fn entry(id: &str, provider: &str) -> CatalogModel {
    CatalogModel {
        id: id.into(),
        name: "Same display name".into(),
        provider: provider.into(),
        model: "same-model".into(),
        ..Default::default()
    }
}

fn sample() -> CatalogUpsert {
    CatalogUpsert {
        models: vec![entry("a", "one"), entry("b", "two")],
        providers: HashMap::from([("one".into(), provider()), ("two".into(), provider())]),
    }
}

#[test]
fn same_named_models_across_providers_have_distinct_directory_ids() {
    let directory = sample();
    validate_catalog(&directory.models, &directory.providers).unwrap();
    let mut duplicate = directory.models.clone();
    duplicate[1].id = "a".into();
    assert!(validate_models(&duplicate).is_err());
}

#[test]
fn omitted_empty_and_literal_none_are_distinct_and_custom_values_round_trip() {
    let raw = r#"
[[models]]
id = "discover"
name = "Discover"
provider = "p"
model = "m"
[[models]]
id = "default"
name = "Default"
provider = "p"
model = "m"
reasoning_efforts = []
[[models]]
id = "explicit"
name = "Explicit"
provider = "p"
model = "m"
reasoning_efforts = ["none", "vendor-deep"]
default_reasoning_effort = "vendor-deep"
"#;
    let parsed: crate::config_toml::ConfigToml = toml::from_str(raw).unwrap();
    assert!(parsed.models[0].reasoning_efforts.is_none());
    assert_eq!(parsed.models[1].reasoning_efforts, Some(Vec::new()));
    assert_eq!(
        parsed.models[2].reasoning_efforts.as_ref().unwrap()[0],
        ReasoningEffort::None
    );
    assert_eq!(
        parsed.models[2]
            .default_reasoning_effort
            .as_ref()
            .unwrap()
            .as_str(),
        "vendor-deep"
    );
    assert!(parsed.models[1].is_user_maintained());
    let encoded = toml::to_string(&parsed.models[2]).unwrap();
    assert_eq!(
        toml::from_str::<CatalogModel>(&encoded).unwrap(),
        parsed.models[2]
    );
}

#[test]
fn old_configuration_has_no_directory_and_keeps_model() {
    let old: crate::config_toml::ConfigToml =
        toml::from_str("model = 'legacy'\nmodel_provider = 'p'\n").unwrap();
    assert!(old.models.is_empty());
    assert_eq!(old.model.as_deref(), Some("legacy"));
}

#[test]
fn discovery_staleness_manual_edits_and_completeness_are_separate() {
    let mut model = entry("a", "p");
    let efforts = vec![ReasoningEffort::High];
    model.reasoning_efforts = Some(efforts.clone());
    model.reasoning_discovery = Some(ReasoningDiscovery {
        source: DiscoverySource::VerifiedProbes,
        discovered_at: 1,
        model: model.model.clone(),
        interface_fingerprint: interface_fingerprint(&provider()),
        complete: false,
        efforts_fingerprint: efforts_fingerprint(&efforts),
    });
    assert!(!model.is_user_maintained());
    let mut edited = model.clone();
    edited.reasoning_efforts = Some(vec![ReasoningEffort::Low]);
    edited.reconcile_discovery(&provider());
    assert!(edited.is_user_maintained());
    assert!(edited.reasoning_discovery.is_none());
    let mut different_endpoint = provider();
    different_endpoint.base_url = Some("http://127.0.0.1:43211/v1".into());
    model.reconcile_discovery(&different_endpoint);
    assert!(model.reasoning_efforts.is_none());
    let mut invalid = edited;
    invalid.reasoning_discovery = Some(ReasoningDiscovery {
        source: DiscoverySource::VerifiedProbes,
        discovered_at: 1,
        model: invalid.model.clone(),
        interface_fingerprint: "x".into(),
        complete: true,
        efforts_fingerprint: "x".into(),
    });
    assert!(validate_models(&[invalid]).is_err());
}

#[test]
fn writes_preserve_comments_and_unrelated_fields_and_reject_stale_versions() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path, "# keep this\nmodel = 'legacy' # keep inline\n").unwrap();
    let old = CatalogSnapshot::load(&path).unwrap();
    let preview = upsert_catalog(&path, &sample(), Some(&old.version), true).unwrap();
    assert!(preview.changed);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "# keep this\nmodel = 'legacy' # keep inline\n"
    );
    upsert_catalog(&path, &sample(), Some(&old.version), false).unwrap();
    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("# keep this"));
    assert!(updated.contains("# keep inline"));
    assert!(updated.contains("model = 'legacy'"));
    assert!(upsert_catalog(&path, &sample(), Some(&old.version), false).is_err());
    assert_eq!(CatalogSnapshot::load(&path).unwrap().models.len(), 2);
}

#[test]
fn configuration_modified_during_edit_is_not_overwritten() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path, "model = 'before'\n").unwrap();
    let result = edit_catalog(&path, None, false, |doc| {
        doc["model"] = toml_edit::value("ours");
        fs::write(&path, "model = 'user-new'\n")?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "model = 'user-new'\n");
}

#[test]
fn discovery_tables_stay_with_their_models_when_merging_parsed_documents() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let mut directory = sample();
    directory.models.push(entry("c", "one"));
    upsert_catalog(&path, &directory, None, false).unwrap();
    let mut raw = fs::read_to_string(&path).unwrap();
    raw.insert_str(0, "# retain settings\n[features]\nmulti_agent = true\n\n");
    fs::write(&path, raw).unwrap();
    let providers = CatalogSnapshot::load(&path).unwrap().providers;

    // Exercise the first, middle and last array entries, then update existing
    // nested tables as well as adding them for the first time.
    for time in [1, 2] {
        for model in &mut directory.models {
            let efforts = vec![ReasoningEffort::High];
            model.reasoning_efforts = Some(efforts.clone());
            model.compatibility = ModelCompatibility::Responses;
            model.reasoning_discovery = Some(ReasoningDiscovery {
                source: DiscoverySource::VerifiedProbes,
                discovered_at: time,
                model: model.model.clone(),
                interface_fingerprint: interface_fingerprint(&providers[&model.provider]),
                complete: false,
                efforts_fingerprint: efforts_fingerprint(&efforts),
            });
            upsert_catalog(
                &path,
                &CatalogUpsert {
                    models: vec![model.clone()],
                    providers: HashMap::new(),
                },
                None,
                false,
            )
            .unwrap();
            let loaded = CatalogSnapshot::load(&path).unwrap();
            assert_eq!(loaded.model(&model.id).unwrap(), model);
            assert_eq!(loaded.models.len(), 3);
        }
    }
    let final_raw = fs::read_to_string(&path).unwrap();
    assert!(final_raw.contains("# retain settings"));
    assert!(final_raw.contains("multi_agent = true"));
}

#[test]
fn named_profile_inherits_providers_and_directory_without_modifying_base() {
    let temp = tempdir().unwrap();
    let base = temp.path().join("config.toml");
    upsert_catalog(&base, &sample(), None, false).unwrap();
    let before = fs::read_to_string(&base).unwrap();
    let profile = temp.path().join("work.config.toml");
    fs::write(&profile, "model = 'profile'\n").unwrap();
    let mut model = entry("c", "one");
    model.reasoning_efforts = Some(Vec::new());
    upsert_catalog(
        &profile,
        &CatalogUpsert {
            models: vec![model],
            providers: HashMap::new(),
        },
        None,
        false,
    )
    .unwrap();
    assert_eq!(CatalogSnapshot::load(&profile).unwrap().models.len(), 3);
    assert_eq!(fs::read_to_string(&base).unwrap(), before);
}

#[test]
fn inline_credentials_are_rejected_and_json_never_exports_existing_secrets() {
    let mut update = sample();
    update
        .providers
        .get_mut("one")
        .unwrap()
        .experimental_bearer_token = Some("secret-value".into());
    assert!(validate_upsert_credentials(&update).is_err());
    let snapshot = CatalogSnapshot {
        models: update.models,
        providers: update.providers,
        version: "v".into(),
        source: "config.toml".into(),
    };
    assert!(
        !snapshot
            .redacted_json()
            .to_string()
            .contains("secret-value")
    );
    assert!(snapshot.redacted_json().to_string().contains("127.0.0.1"));
}

#[test]
fn absent_credentials_have_explicit_status_without_network() {
    let mut providers = sample().providers;
    providers.get_mut("one").unwrap().env_key =
        Some("XDCODE_TEST_NONEXISTENT_MODEL_CREDENTIAL_574F".into());
    let snapshot = CatalogSnapshot {
        models: sample().models,
        providers,
        version: "v".into(),
        source: "config.toml".into(),
    };
    let checked = static_check(&snapshot, "a").unwrap();
    assert_eq!(checked.status, ModelStatus::MissingCredentials);
    assert_eq!(
        checked.missing_credentials,
        vec!["XDCODE_TEST_NONEXISTENT_MODEL_CREDENTIAL_574F"]
    );
}

#[cfg(unix)]
#[test]
fn read_only_configuration_rejects_writes_even_as_root() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let path = temp.path().join("config.toml");
    fs::write(&path, "# protected\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    assert!(upsert_catalog(&path, &sample(), None, false).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "# protected\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
}

#[tokio::test]
async fn manual_effort_lists_are_retained_without_network_or_credentials() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let mut update = sample();
    update.models[0].reasoning_efforts = Some(vec![ReasoningEffort::Custom("vendor".into())]);
    update.providers.get_mut("one").unwrap().env_key =
        Some("XDCODE_TEST_NONEXISTENT_MODEL_CREDENTIAL_574F".into());
    upsert_catalog(&path, &update, None, false).unwrap();
    let result = refresh_reasoning(&path, "a", false).await.unwrap();
    assert!(result.saved);
    assert_eq!(
        result.model.reasoning_efforts,
        update.models[0].reasoning_efforts
    );
}

#[test]
fn endpoint_identity_supports_provider_reuse_without_exporting_url_credentials() {
    let value =
        redacted_endpoint("https://user:secret@openrouter.ai/api/v1?api_key=private#fragment");
    assert_eq!(value["origin"], "https://openrouter.ai");
    assert_eq!(value["path"], "/api/v1");
    assert!(!value.to_string().contains("secret"));
    assert!(!value.to_string().contains("private"));
    let hidden = redacted_endpoint("https://example.test/private-token-embedded-in-path");
    assert_eq!(hidden["path"], "<redacted>");
    assert!(!hidden.to_string().contains("private-token"));
    assert_ne!(
        redacted_endpoint("https://example.test/v1")["fingerprint"],
        redacted_endpoint("https://example.test/api/v1")["fingerprint"]
    );
}

fn stack(layers: Vec<crate::ConfigLayerEntry>) -> crate::ConfigLayerStack {
    crate::ConfigLayerStack::new(
        layers,
        crate::ConfigRequirements::default(),
        crate::ConfigRequirementsToml::default(),
    )
    .unwrap()
}

fn absolute(path: &Path) -> xdcode_utils_absolute_path::AbsolutePathBuf {
    xdcode_utils_absolute_path::AbsolutePathBuf::from_absolute_path(path.to_owned()).unwrap()
}

#[test]
fn layered_upsert_writes_effective_project_directory_using_inherited_provider() {
    let temporary = tempdir().unwrap();
    let user = temporary.path().join("config.toml");
    upsert_catalog(&user, &sample(), None, false).unwrap();
    let user_before = fs::read_to_string(&user).unwrap();
    let project_folder = temporary.path().join("project/.xdcode");
    fs::create_dir_all(&project_folder).unwrap();
    let project = project_folder.join("config.toml");
    fs::write(
        &project,
        format!(
            "[[models]]\n{}",
            toml::to_string(&entry("project", "one")).unwrap()
        ),
    )
    .unwrap();
    let layers = stack(vec![
        crate::ConfigLayerEntry::new(
            crate::ConfigLayerSource::User {
                file: absolute(&user),
                profile: None,
            },
            toml::from_str(&user_before).unwrap(),
        ),
        crate::ConfigLayerEntry::new(
            crate::ConfigLayerSource::Project {
                dot_xdcode_folder: absolute(&project_folder),
            },
            toml::from_str(&fs::read_to_string(&project).unwrap()).unwrap(),
        ),
    ]);
    let before = CatalogSnapshot::load_layered(&layers).unwrap();
    assert_eq!(before.source, project);
    assert_eq!(before.models.len(), 1);
    let new_model = entry("new", "one");
    upsert_catalog_layered(
        &layers,
        &CatalogUpsert {
            models: vec![new_model],
            providers: HashMap::new(),
        },
        Some(&before.version),
        false,
    )
    .unwrap();
    let after = CatalogSnapshot::load_layered(&layers).unwrap();
    assert_eq!(after.models.len(), 2);
    assert!(after.models.iter().any(|model| model.id == "new"));
    assert_eq!(fs::read_to_string(&user).unwrap(), user_before);
}

#[test]
fn model_upsert_refuses_session_shadowed_directory() {
    let temporary = tempdir().unwrap();
    let user = temporary.path().join("config.toml");
    upsert_catalog(&user, &sample(), None, false).unwrap();
    let layers = stack(vec![
        crate::ConfigLayerEntry::new(
            crate::ConfigLayerSource::User {
                file: absolute(&user),
                profile: None,
            },
            toml::from_str(&fs::read_to_string(&user).unwrap()).unwrap(),
        ),
        crate::ConfigLayerEntry::new(
            crate::ConfigLayerSource::SessionFlags,
            toml::from_str("models = []").unwrap(),
        ),
    ]);
    assert!(upsert_catalog_layered(&layers, &sample(), None, false).is_err());
}

#[test]
fn provider_patch_preserves_unrelated_capability_flags_and_comments() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("config.toml");
    fs::write(&path,"[model_providers.one]\nname = 'One'\nbase_url = 'http://localhost:43210/v1'\nsupports_websockets = true # preserve capability\n").unwrap();
    upsert_catalog(
        &path,
        &CatalogUpsert {
            models: vec![entry("one", "one")],
            providers: HashMap::from([(
                "one".into(),
                ModelProviderInfo {
                    name: "Renamed".into(),
                    ..Default::default()
                },
            )]),
        },
        None,
        false,
    )
    .unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("supports_websockets = true # preserve capability"));
    assert!(after.contains("http://localhost:43210/v1"));
    assert_eq!(
        CatalogSnapshot::load(&path).unwrap().providers["one"].name,
        "Renamed"
    );
}
