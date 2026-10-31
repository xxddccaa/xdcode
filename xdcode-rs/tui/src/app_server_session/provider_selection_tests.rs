//! Exercise provider defaults and explicit choices through real app-server requests.

use super::*;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn starts_and_forks_use_server_provider_unless_explicitly_selected() -> Result<()> {
    let home = tempfile::tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
model_provider = "server-provider"
model = "gpt-5.2"
[model_providers.server-provider]
name = "Server provider"
base_url = "http://127.0.0.1:9/v1"
wire_api = "responses"
requires_hosted_auth = false
[model_providers.client-provider]
name = "Client provider"
base_url = "http://127.0.0.1:9/v1"
wire_api = "responses"
requires_hosted_auth = false
"#,
    )?;
    let server_config = ConfigBuilder::default()
        .xdcode_home(home.path().to_path_buf())
        .build()
        .await?;
    let client_config = ConfigBuilder::default()
        .xdcode_home(home.path().to_path_buf())
        .harness_overrides(ConfigOverrides {
            model_provider: Some("client-provider".into()),
            ..Default::default()
        })
        .build()
        .await?;
    let local_settings = LocalSettings::from(&client_config);
    let parent = crate::tests::write_session_rollout(
        home.path(),
        "2025-01-02T10-00-00",
        "2025-01-02T10:00:00Z",
        "Server history",
        "server-provider",
        server_config.cwd.as_path(),
    )?;
    let mut server = crate::start_embedded_app_server_for_picker(&server_config).await?;
    assert_eq!(
        crate::lookup_session_target_with_app_server(&mut server, &client_config, "Server history")
            .await?
            .unwrap()
            .thread_id,
        parent
    );
    let resumed = server
        .resume_thread(
            &local_settings,
            client_config.clone(),
            parent,
            ResumeModelSettings::OverrideFromCurrentConfig,
        )
        .await?;
    assert_eq!(resumed.session.model_provider_id, "server-provider");
    for explicit in [false, true] {
        server.model_provider_override = explicit.then(|| "client-provider".to_string());
        let expected = if explicit {
            "client-provider"
        } else {
            "server-provider"
        };
        let started = server.start_thread(&client_config).await?;
        let forked = server
            .fork_thread(&local_settings, client_config.clone(), parent)
            .await?;
        assert_eq!(
            (
                started.session.model_provider_id.as_str(),
                forked.session.model_provider_id.as_str()
            ),
            (expected, expected)
        );
    }
    server.model_provider_override = None;
    let path = home.path().join("config.toml");
    let updated = std::fs::read_to_string(&path)?.replacen(
        "model_provider = \"server-provider\"",
        "model_provider = \"client-provider\"",
        1,
    );
    std::fs::write(path, updated)?;
    assert_eq!(server.history_model_provider(&client_config).await?, None);
    assert_eq!(
        server
            .start_thread(&client_config)
            .await?
            .session
            .model_provider_id,
        "client-provider"
    );
    // A fork of the active session preserves its provider even if launch defaults differ.
    server.model_provider_override = Some("client-provider".into());
    let forked = server
        .fork_thread_at(
            &local_settings,
            server_config,
            parent,
            /*last_turn_id*/ None,
            /*before_turn_id*/ None,
            ForkGoalContinuation::StartIfIdle,
            /*selected_profile*/ None,
        )
        .await?;
    assert_eq!(forked.session.model_provider_id, "server-provider");
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn selected_profile_provider_is_explicit() -> Result<()> {
    let home = tempfile::tempdir()?;
    let profile_path = AbsolutePathBuf::try_from(home.path().join("selected.config.toml"))?;
    let fixture = crate::tests::custom_provider_config();
    std::fs::write(&profile_path, toml::to_string(&fixture)?)?;
    let config = ConfigBuilder::default()
        .xdcode_home(home.path().to_path_buf())
        .loader_overrides(xdcode_config::LoaderOverrides {
            user_config_path: Some(profile_path),
            user_config_profile: Some("selected".parse()?),
            ..xdcode_config::LoaderOverrides::without_managed_config_for_tests()
        })
        .build()
        .await?;
    assert_eq!(
        provider_selection::explicit_provider(&config),
        Some("test-provider".into())
    );
    Ok(())
}

#[tokio::test]
async fn required_inference_provider_does_not_filter_history() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut fixture = crate::tests::custom_provider_config();
    let provider = fixture.model_providers.remove("test-provider").unwrap();
    fixture
        .model_providers
        .insert("required-provider".into(), provider.clone());
    fixture
        .model_providers
        .insert("other-provider".into(), provider);
    fixture.model_provider = Some("required-provider".into());
    std::fs::write(home.path().join("config.toml"), toml::to_string(&fixture)?)?;
    std::fs::write(
        home.path().join("requirements.toml"),
        "model_provider = 'required-provider'",
    )?;
    let config = ConfigBuilder::default()
        .xdcode_home(home.path().to_path_buf())
        .loader_overrides(
            xdcode_config::LoaderOverrides::with_managed_config_path_for_tests(
                home.path().join("managed_config.toml"),
            ),
        )
        .harness_overrides(ConfigOverrides {
            model_provider: Some("other-provider".into()),
            ..Default::default()
        })
        .build()
        .await?;
    let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
    server.model_provider_override = Some("other-provider".into());
    assert_eq!(config.model_provider_id, "required-provider");
    assert_eq!(server.history_model_provider(&config).await?, None);
    server.shutdown().await?;
    Ok(())
}

/// Profiles select inference routes while sharing the home's existing history store.
#[tokio::test]
async fn ds_and_deepkey_profiles_share_named_and_latest_history() -> Result<()> {
    let home = tempfile::tempdir()?;
    let project = home.path().join("project");
    let other_project = home.path().join("other-project");
    std::fs::create_dir_all(&project)?;
    std::fs::create_dir_all(&other_project)?;
    let mut expected_ids = Vec::new();
    for (provider, hour, cwd) in [
        ("ds", "10", &project),
        ("deepkey", "11", &project),
        ("ds", "12", &other_project),
    ] {
        let id = crate::tests::write_session_rollout(
            home.path(),
            &format!("2025-01-02T{hour}-00-00"),
            &format!("2025-01-02T{hour}:00:00Z"),
            &format!("{provider} history {hour}"),
            provider,
            cwd,
        )?;
        if cwd == &project {
            expected_ids.push(id.to_string());
        }
    }
    expected_ids.sort();
    for provider in ["ds", "deepkey"] {
        let path = home.path().join(format!("{provider}.config.toml"));
        std::fs::write(
            &path,
            format!(
                "model_provider = '{provider}'\nmodel = 'shared-model'\n\
                 [model_providers.{provider}]\nname = '{provider}'\n\
                 base_url = 'http://127.0.0.1:9/{provider}/v1'\n\
                 wire_api = 'responses'\nrequires_hosted_auth = false\n"
            ),
        )?;
        let config = ConfigBuilder::default()
            .xdcode_home(home.path().to_path_buf())
            .loader_overrides(xdcode_config::LoaderOverrides {
                user_config_path: Some(AbsolutePathBuf::try_from(path)?),
                user_config_profile: Some(provider.parse()?),
                ..xdcode_config::LoaderOverrides::without_managed_config_for_tests()
            })
            .harness_overrides(ConfigOverrides {
                cwd: Some(project.clone()),
                ..Default::default()
            })
            .build()
            .await?;
        assert_eq!(
            provider_selection::explicit_provider(&config).as_deref(),
            Some(provider)
        );
        let mut server = crate::start_embedded_app_server_for_picker(&config).await?;
        let history_provider = server.history_model_provider(&config).await?;
        let mut params = crate::latest_session_lookup_params(
            history_provider,
            Some(&project),
            false,
            crate::LatestSessionLookupMode::ScanAndRepair,
        );
        assert_eq!(params.model_providers, Some(Vec::new()));
        params.limit = Some(100);
        let mut found_ids: Vec<_> = server
            .thread_list(params)
            .await?
            .data
            .into_iter()
            .map(|thread| thread.id)
            .collect();
        found_ids.sort();
        assert_eq!(
            found_ids, expected_ids,
            "both profiles must see both providers in this project"
        );
        let opposite_label = if provider == "ds" {
            "deepkey history 11"
        } else {
            "ds history 10"
        };
        assert!(
            crate::lookup_session_target_with_app_server(&mut server, &config, opposite_label)
                .await?
                .is_some(),
            "named resume must find the other provider"
        );
        let latest = crate::lookup_latest_session_target_with_app_server(
            &mut server,
            &config,
            Some(&project),
            false,
        )
        .await?
        .expect("shared latest history");
        let expected_latest = crate::lookup_session_target_with_app_server(
            &mut server,
            &config,
            "deepkey history 11",
        )
        .await?
        .expect("latest project session");
        assert_eq!(latest.thread_id, expected_latest.thread_id);
        server.shutdown().await?;
    }
    Ok(())
}
