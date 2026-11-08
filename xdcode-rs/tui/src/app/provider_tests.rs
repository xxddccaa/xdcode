use super::*;

fn catalog() -> Catalog {
    Catalog {
        providers: vec![Provider {
            id: "custom".into(),
            name: "Custom provider".into(),
            values: [
                (Field::Name, "Custom provider".into()),
                (Field::Url, "https://example.invalid/v1".into()),
                (Field::Variable, "CUSTOM_API_KEY".into()),
            ]
            .into(),
            editable: true,
        }],
        file: Some("/tmp/config.toml".into()),
        version: Some("original-version".into()),
    }
}

#[test]
fn provider_edits_write_only_changed_fields_without_switching_defaults() {
    let catalog = catalog();
    let draft = Draft {
        provider: Some(catalog.providers[0].clone()),
        catalog,
        changes: [(Field::Name, "Renamed provider".into())].into(),
    };
    let patch = draft.patch().expect("valid provider edit");
    assert_eq!(patch.edits.len(), 1);
    assert_eq!(patch.edits[0].key_path, "model_providers.\"custom\".name");
    assert_eq!(patch.expected_version.as_deref(), Some("original-version"));
    assert!(!patch.reload_user_config);
}

#[test]
fn conflicts_rebase_metadata_but_keep_the_unsaved_draft() {
    let mut catalog = catalog();
    let mut draft = Draft {
        provider: Some(catalog.providers[0].clone()),
        catalog: catalog.clone(),
        changes: [(Field::Url, "https://new.example.invalid/v1".into())].into(),
    };
    catalog.version = Some("new-version".into());
    draft.rebase(catalog);
    let patch = draft.patch().expect("explicit retry");
    assert_eq!(patch.expected_version.as_deref(), Some("new-version"));
    assert_eq!(
        patch.edits[0].value,
        serde_json::json!("https://new.example.invalid/v1")
    );
}

#[test]
fn read_only_providers_and_embedded_credentials_are_rejected() {
    let catalog = catalog();
    let mut draft = Draft {
        provider: Some(catalog.providers[0].clone()),
        catalog,
        changes: [(Field::Url, "https://user:secret@example.invalid/v1".into())].into(),
    };
    assert!(draft.patch().is_err());
    draft
        .changes
        .insert(Field::Url, "https://example.invalid/v1".into());
    draft.provider.as_mut().expect("provider").editable = false;
    assert!(draft.patch().is_err());
}

#[test]
fn catalog_discards_secret_values_and_preserves_credential_references() {
    let response: ConfigReadResponse = serde_json::from_value(serde_json::json!({
        "config": { "model_providers": { "custom": { "name": "Custom", "wire_api": "responses", "env_key": "CUSTOM_API_KEY",
            "experimental_bearer_token": "not-a-real-key", "http_headers": { "Authorization": "not-a-real-header" } } } },
        "origins": {}, "layers": []
    })).expect("configuration response");
    let catalog = Catalog::from_response(response);
    assert_eq!(
        catalog.providers[0]
            .values
            .get(&Field::Variable)
            .map(String::as_str),
        Some("CUSTOM_API_KEY")
    );
    assert!(!format!("{catalog:?}").contains("not-a-real"));
}
