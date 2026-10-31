use super::super::env_value;
use super::CredentialHostBinding;
use super::CredentialProvider;
use super::CredentialSource;
use super::shaped_dummy_value;
use crate::config::trusted_credential_broker_host;
use rama_http::HeaderMap;
use rama_http::HeaderValue;
use rama_http::header::AUTHORIZATION;
use std::collections::HashMap;

const PROVIDER_API_KEY_ENV_VARS: &[&str] = &["PROVIDER_API_KEY"];
const PROVIDER_API_KEY_PREFIXES: &[&str] = &["sk-proj-", "sk-svcacct-", "sk-admin-", "sk-"];
const PROVIDER_BASE_URL_ENV_VAR: &str = "PROVIDER_BASE_URL";
const PROVIDER_API_KEY_MIN_LEN: usize = 51;
const PROVIDER_API_HOST: &str = "api.openai.com";

pub(super) static PROVIDER: CredentialProvider = CredentialProvider {
    context_env_vars: &[PROVIDER_BASE_URL_ENV_VAR],
    credential_prefixes: PROVIDER_API_KEY_PREFIXES,
    ignored_credential_prefixes: &["sk-ant-", "sk-or-"],
    credential_watermark: Some("T3BlbkFJ"),
    minimum_credential_len: PROVIDER_API_KEY_MIN_LEN,
    sources: &[CredentialSource {
        env_vars: PROVIDER_API_KEY_ENV_VARS,
        binding_env_vars: &[PROVIDER_BASE_URL_ENV_VAR],
        invalidates_host_binding: |env| {
            env_value(env, PROVIDER_BASE_URL_ENV_VAR)
                .is_some_and(|value| trusted_credential_broker_host(value).is_none())
        },
        host_binding,
    }],
    reset_on_configuration_change: true,
    dummy_value,
    translate_request_header,
    request_header_value,
    insert_request_header,
};

fn dummy_value(real_value: &str) -> String {
    shaped_dummy_value(
        real_value,
        provider_api_key_prefix(real_value),
        PROVIDER_API_KEY_MIN_LEN,
    )
}

fn translate_request_header(
    headers: &HeaderMap,
    expected_value: &str,
    replacement_value: &str,
) -> Option<HeaderValue> {
    headers
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .filter(|header| header.contains(expected_value))?;
    request_header_value(replacement_value)
}

fn request_header_value(value: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!("Bearer {value}")).ok()
}

fn insert_request_header(headers: &mut HeaderMap, value: HeaderValue) {
    headers.insert(AUTHORIZATION, value);
}

fn host_binding(
    env: &HashMap<String, String>,
    configured_host: Option<&str>,
) -> Option<CredentialHostBinding> {
    let mut hosts = vec![PROVIDER_API_HOST.to_string()];
    for host in configured_host
        .map(str::to_string)
        .into_iter()
        .chain(env_value(env, PROVIDER_BASE_URL_ENV_VAR).and_then(trusted_credential_broker_host))
    {
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }

    Some(match hosts.as_slice() {
        [host] => CredentialHostBinding::ExactHost(host.clone()),
        _ => CredentialHostBinding::ExactHosts(hosts),
    })
}

fn provider_api_key_prefix(value: &str) -> &str {
    PROVIDER_API_KEY_PREFIXES
        .iter()
        .copied()
        .find(|prefix| value.starts_with(prefix))
        .unwrap_or("sk-")
}
