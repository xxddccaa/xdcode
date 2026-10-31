use tempfile::TempDir;
use xdcode_utils_absolute_path::AbsolutePathBuf;

use super::has_only_search_config_override;
use super::should_delay_startup_composer_for_first_login;

#[test]
fn startup_delays_composer_for_homes_without_authentication_state() -> std::io::Result<()> {
    let temporary_directory = TempDir::new()?;
    let xdcode_home = temporary_directory.path().join("xdcode-home");
    let system_config_path =
        AbsolutePathBuf::from_absolute_path(temporary_directory.path().join("system.toml"))?;

    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |_| None,
    ));
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |name| (name == xdcode_login::XDCODE_ACCESS_TOKEN_ENV_VAR).then(|| "  ".into()),
    ));

    std::fs::create_dir_all(xdcode_home.join("tmp").join("arg0"))?;
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |_| None,
    ));
    let helper_directory = xdcode_home
        .join("tmp")
        .join("arg0")
        .join("xdcode-arg0-session");
    std::fs::create_dir(&helper_directory)?;
    std::fs::write(helper_directory.join("apply_patch"), "")?;
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |_| None,
    ));

    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |name| (name == "XDCODE_HOME").then(|| "/custom/home".into()),
    ));
    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |name| {
            (name == xdcode_login::XDCODE_ACCESS_TOKEN_ENV_VAR).then(|| "access-token".into())
        },
    ));
    for disabled_credential in [
        xdcode_login::PROVIDER_API_KEY_ENV_VAR,
        xdcode_login::XDCODE_API_KEY_ENV_VAR,
    ] {
        assert!(should_delay_startup_composer_for_first_login(
            &xdcode_home,
            Ok(system_config_path.clone()),
            || Ok(false),
            |name| (name == disabled_credential).then(|| "disabled-key".into()),
        ));
    }

    for existing_state in ["history.jsonl", "log", "sessions"] {
        let state_path = xdcode_home.join(existing_state);
        if existing_state == "history.jsonl" {
            std::fs::write(&state_path, "")?;
        } else {
            std::fs::create_dir(&state_path)?;
        }
        assert!(should_delay_startup_composer_for_first_login(
            &xdcode_home,
            Ok(system_config_path.clone()),
            || Ok(false),
            |_| None,
        ));
    }
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |name| (name == "XDCODE_HOME").then(|| "/custom/home".into()),
    ));
    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || panic!("process credentials should not probe managed configuration"),
        |name| match name {
            "XDCODE_HOME" => Some("/custom/home".into()),
            xdcode_login::XDCODE_ACCESS_TOKEN_ENV_VAR => Some("access-token".into()),
            _ => None,
        },
    ));

    for existing_state in ["auth.json", "config.toml", "environments.toml"] {
        let state_path = xdcode_home.join(existing_state);
        std::fs::write(&state_path, "")?;
        assert!(!should_delay_startup_composer_for_first_login(
            &xdcode_home,
            Ok(system_config_path.clone()),
            || panic!("configured homes should not probe managed configuration"),
            |_| None,
        ));
        std::fs::remove_file(state_path)?;
    }

    let daemon_directory = xdcode_home.join("app-server-control");
    std::fs::create_dir(&daemon_directory)?;
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |_| None,
    ));
    std::fs::write(daemon_directory.join("app-server-control.sock"), "")?;
    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || panic!("daemon-owned homes should not probe managed configuration"),
        |_| None,
    ));
    std::fs::remove_file(daemon_directory.join("app-server-control.sock"))?;
    std::fs::remove_dir(daemon_directory)?;

    let additional_temporary_state = xdcode_home.join("tmp").join("other");
    std::fs::write(&additional_temporary_state, "")?;
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(false),
        |_| None,
    ));

    let invalid_home = temporary_directory.path().join("invalid-home");
    std::fs::write(&invalid_home, "not a directory")?;
    assert!(!should_delay_startup_composer_for_first_login(
        &invalid_home,
        Ok(system_config_path),
        || Ok(false),
        |_| None,
    ));
    Ok(())
}

#[test]
fn startup_keeps_composer_when_home_state_cannot_be_confirmed() -> std::io::Result<()> {
    let temporary_directory = TempDir::new()?;
    let xdcode_home = temporary_directory.path().join("xdcode-home");
    let system_config_path =
        AbsolutePathBuf::from_absolute_path(temporary_directory.path().join("system.toml"))?;

    std::fs::create_dir(&xdcode_home)?;
    assert!(should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path),
        || Ok(false),
        |_| None,
    ));

    #[cfg(unix)]
    {
        let system_config_path =
            AbsolutePathBuf::from_absolute_path(temporary_directory.path().join("system.toml"))?;
        let daemon_directory = xdcode_home.join("app-server-control");
        std::fs::write(&daemon_directory, "not a directory")?;
        assert!(!should_delay_startup_composer_for_first_login(
            &xdcode_home,
            Ok(system_config_path.clone()),
            || panic!("ambiguous daemon state should not probe managed configuration"),
            |_| None,
        ));
        std::fs::remove_file(&daemon_directory)?;

        let blocking_parent = temporary_directory.path().join("blocking-home-parent");
        std::fs::write(&blocking_parent, "not a directory")?;
        assert!(!should_delay_startup_composer_for_first_login(
            &blocking_parent.join("xdcode-home"),
            Ok(system_config_path),
            || panic!("ambiguous home state should not probe managed configuration"),
            |_| None,
        ));
    }
    Ok(())
}

#[test]
fn startup_keeps_composer_for_workload_identity_markers() -> std::io::Result<()> {
    let temporary_directory = TempDir::new()?;
    let xdcode_home = temporary_directory.path().join("xdcode-home");
    let system_config_path =
        AbsolutePathBuf::from_absolute_path(temporary_directory.path().join("system.toml"))?;

    for marker in [
        xdcode_protocol::shell_environment::PROVIDER_FEDERATION_RULE_ID_ENV_VAR,
        xdcode_protocol::shell_environment::PROVIDER_IDENTITY_TOKEN_FILE_ENV_VAR,
    ] {
        for value in ["", "configured"] {
            assert!(!should_delay_startup_composer_for_first_login(
                &xdcode_home,
                Ok(system_config_path.clone()),
                || panic!("workload identity should not probe managed configuration"),
                |name| (name == marker).then(|| value.into()),
            ));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;

        assert!(!should_delay_startup_composer_for_first_login(
            &xdcode_home,
            Ok(system_config_path),
            || panic!("workload identity should not probe managed configuration"),
            |name| {
                (name == xdcode_protocol::shell_environment::PROVIDER_IDENTITY_TOKEN_FILE_ENV_VAR)
                    .then(|| std::ffi::OsString::from_vec(vec![b'/', 0xff]))
            },
        ));
    }
    Ok(())
}

#[test]
fn startup_only_accepts_the_synthetic_search_override() {
    let search = (
        "web_search".to_string(),
        toml::Value::String("live".to_string()),
    );

    assert!(!has_only_search_config_override(&[]));
    assert!(has_only_search_config_override(std::slice::from_ref(
        &search
    )));
    assert!(!has_only_search_config_override(&[(
        "web_search".to_string(),
        toml::Value::String("cached".to_string()),
    )]));
    assert!(!has_only_search_config_override(&[(
        "model_provider".to_string(),
        toml::Value::String("live".to_string()),
    )]));
    assert!(!has_only_search_config_override(&[
        search,
        (
            "model".to_string(),
            toml::Value::String("custom".to_string())
        ),
    ]));
}

#[test]
fn startup_keeps_composer_when_system_configuration_is_possible() -> std::io::Result<()> {
    let temporary_directory = TempDir::new()?;
    let xdcode_home = temporary_directory.path().join("xdcode-home");
    let system_config_path =
        AbsolutePathBuf::from_absolute_path(temporary_directory.path().join("system.toml"))?;

    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Err(std::io::Error::other(
            "system configuration path is unavailable"
        )),
        || Ok(false),
        |_| None,
    ));

    #[cfg(unix)]
    {
        let blocking_parent = temporary_directory.path().join("blocking-parent");
        std::fs::write(&blocking_parent, "not a directory")?;
        assert!(!should_delay_startup_composer_for_first_login(
            &xdcode_home,
            AbsolutePathBuf::from_absolute_path(blocking_parent.join("system.toml")),
            || Ok(false),
            |_| None,
        ));
    }

    std::fs::write(system_config_path.as_path(), "model_provider = 'local'")?;
    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path),
        || Ok(false),
        |_| None,
    ));
    Ok(())
}

#[test]
fn startup_keeps_composer_when_managed_configuration_is_possible() -> std::io::Result<()> {
    let temporary_directory = TempDir::new()?;
    let xdcode_home = temporary_directory.path().join("xdcode-home");
    let system_config_path =
        AbsolutePathBuf::from_absolute_path(temporary_directory.path().join("system.toml"))?;

    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path.clone()),
        || Ok(true),
        |_| None,
    ));
    assert!(!should_delay_startup_composer_for_first_login(
        &xdcode_home,
        Ok(system_config_path),
        || Err(std::io::Error::other(
            "managed configuration is inaccessible"
        )),
        |_| None,
    ));
    Ok(())
}
