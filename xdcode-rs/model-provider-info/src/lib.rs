//! Registry of model providers supported by Xdcode.
//!
//! Providers are user-defined entries inside `~/.xdcode/config.toml` under
//! `model_providers`. The built-in `custom` entry is an endpoint-less setup
//! placeholder; it cannot send requests until the user replaces it.
//!
//! API provider construction applies the process-wide managed residency policy also
//! used by default HTTP headers.

use http::HeaderMap;
use http::header::HeaderName;
use http::header::HeaderValue;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::path::Component;
use std::path::Path;
use std::sync::PoisonError;
use std::sync::RwLock;
use std::time::Duration;
use xdcode_client::Provider as ApiProvider;
use xdcode_client::RetryConfig as ApiRetryConfig;
use xdcode_protocol::auth::AuthMode;
use xdcode_protocol::config_types::ModelProviderAuthInfo;
use xdcode_protocol::error::EnvVarError;
use xdcode_protocol::error::Result as XdcodeResult;
use xdcode_protocol::error::XdcodeErr;
use xdcode_utils_redacted_string::RedactedString;

mod capabilities;
mod gateway_oauth;
mod oauth_session;
pub use capabilities::ModelProviderCapabilities;
pub use capabilities::RemoteCompactionSupport;
pub use gateway_oauth::GatewayOAuthConfig;
pub use gateway_oauth::GatewayOAuthDelivery;
pub use oauth_session::OAuthSessionConfig;

pub const RESIDENCY_HEADER_NAME: &str = "x-openai-internal-codex-residency";

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResidencyRequirement {
    Us,
}

static REQUIREMENTS_RESIDENCY: RwLock<Option<ResidencyRequirement>> = RwLock::new(None);

/// Sets the process-wide residency requirement loaded from managed configuration.
pub fn set_managed_residency_requirement(enforce_residency: Option<ResidencyRequirement>) {
    // Recover the stored policy if the lock is poisoned rather than silently disabling it.
    *REQUIREMENTS_RESIDENCY
        .write()
        .unwrap_or_else(PoisonError::into_inner) = enforce_residency;
}

/// Returns the current process-wide managed residency requirement.
pub fn read_managed_residency_requirement() -> Option<ResidencyRequirement> {
    *REQUIREMENTS_RESIDENCY
        .read()
        .unwrap_or_else(PoisonError::into_inner)
}

const DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_STREAM_MAX_RETRIES: u64 = 5;
const DEFAULT_REQUEST_MAX_RETRIES: u64 = 4;
const DEFAULT_AWS_CREDENTIAL_EXPORT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_AWS_AUTH_REFRESH_TIMEOUT_MS: u64 = 300_000;
pub const DEFAULT_WEBSOCKET_CONNECT_TIMEOUT_MS: u64 = 15_000;
/// Hard cap for user-configured `stream_max_retries`.
const MAX_STREAM_MAX_RETRIES: u64 = 100;
/// Hard cap for user-configured `request_max_retries`.
const MAX_REQUEST_MAX_RETRIES: u64 = 100;

const PROVIDER_ACTOR_AUTHORIZATION_HEADER: &str = "x-openai-actor-authorization";
pub const PROVIDER_PROVIDER_ID: &str = "provider";
pub const CUSTOM_PROVIDER_ID: &str = "custom";
pub const CUSTOM_PROVIDER_SETUP: &str = "xdcode requires a user-configured Responses provider. Set model and model_provider in ~/.xdcode/config.toml, add [model_providers.<name>] with base_url, wire_api = \"responses\" and env_key = \"XDCODE_API_KEY\", then export XDCODE_API_KEY. Hosted login is unavailable.";
pub const CHATGPT_XDCODE_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const AMAZON_BEDROCK_PROVIDER_NAME: &str = "Amazon Bedrock";
pub const AMAZON_BEDROCK_PROVIDER_ID: &str = "amazon-bedrock";
const AMAZON_BEDROCK_RUNTIME_PROVIDER_NAME: &str = "Amazon Bedrock Runtime";
pub const AMAZON_BEDROCK_RUNTIME_PROVIDER_ID: &str = "amazon-bedrock-runtime";
const GCP_PROVIDER_NAME: &str = "Google Cloud Vertex AI";
pub const GCP_PROVIDER_ID: &str = "gcp";
pub const AMAZON_BEDROCK_GPT_5_5_MODEL_ID: &str = "openai.gpt-5.5";
pub const AMAZON_BEDROCK_GPT_5_6_SOL_MODEL_ID: &str = "openai.gpt-5.6-sol";
pub const AMAZON_BEDROCK_GPT_6_1_SOL_MODEL_ID: &str = "openai.gpt-6.1-sol";
pub const AMAZON_BEDROCK_GPT_6_1_TERRA_MODEL_ID: &str = "openai.gpt-6.1-terra";
pub const AMAZON_BEDROCK_GPT_6_SOL_MODEL_ID: &str = "openai.gpt-6-sol";
pub const AMAZON_BEDROCK_GPT_6_LUNA_MODEL_ID: &str = "openai.gpt-6-luna";
pub const AMAZON_BEDROCK_GPT_6_ASTRA_MODEL_ID: &str = "openai.gpt-6-astra";
pub const AMAZON_BEDROCK_GPT_5_6_TERRA_MODEL_ID: &str = "openai.gpt-5.6-terra";
pub const AMAZON_BEDROCK_GPT_5_6_LUNA_MODEL_ID: &str = "openai.gpt-5.6-luna";
pub const AMAZON_BEDROCK_RUNTIME_GLOBAL_GPT_5_6_TERRA_MODEL_ID: &str =
    "global.openai.gpt-5.6-terra";
pub const AMAZON_BEDROCK_RUNTIME_GLOBAL_GPT_5_6_LUNA_MODEL_ID: &str = "global.openai.gpt-5.6-luna";
pub const AMAZON_BEDROCK_DEFAULT_BASE_URL: &str =
    "https://bedrock-mantle.us-east-1.api.aws/openai/v1";
const AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_HEADER: &str = "x-amzn-mantle-client-agent";
const AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_VALUE: &str = "xdcode";
const CHAT_WIRE_API_REMOVED_ERROR: &str = "`wire_api = \"chat\"` is no longer supported.\nSet `wire_api = \"responses\"` in your provider config.";
pub const LEGACY_OLLAMA_CHAT_PROVIDER_ID: &str = "ollama-chat";
pub const OLLAMA_CHAT_PROVIDER_REMOVED_ERROR: &str = "`ollama-chat` is no longer supported.\nConfigure a custom Responses-compatible endpoint in `model_providers`.";

/// Wire protocol that the provider speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WireApi {
    /// The Responses API exposed by Provider at `/v1/responses`.
    #[default]
    Responses,
}

impl fmt::Display for WireApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Responses => "responses",
        };
        f.write_str(value)
    }
}

impl<'de> Deserialize<'de> for WireApi {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "responses" => Ok(Self::Responses),
            "chat" => Err(serde::de::Error::custom(CHAT_WIRE_API_REMOVED_ERROR)),
            _ => Err(serde::de::Error::unknown_variant(&value, &["responses"])),
        }
    }
}

/// Serializable representation of a provider definition.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ModelProviderInfo {
    /// Friendly display name.
    #[serde(default)]
    pub name: String,
    /// Base URL for the provider's Provider-compatible API.
    pub base_url: Option<String>,
    /// Optional full URL for a Xdcode-native model catalog. When unset, Provider discovery
    /// uses the Xdcode backend unless `base_url` overrides the inference endpoint.
    pub model_catalog_url: Option<RedactedString>,
    /// Environment variable that stores the user's API key for this provider.
    pub env_key: Option<String>,

    /// Optional instructions to help the user get a valid value for the
    /// variable and set it.
    pub env_key_instructions: Option<String>,
    /// Value to use with `Authorization: Bearer <token>` header. Use of this
    /// config is discouraged in favor of `env_key` for security reasons, but
    /// this may be necessary when using this programmatically.
    pub experimental_bearer_token: Option<RedactedString>,
    /// Command-backed bearer-token configuration for this provider.
    pub auth: Option<ModelProviderAuthInfo>,
    /// Secondary OAuth credentials required by the provider's gateway.
    pub gateway_oauth: Option<GatewayOAuthConfig>,
    /// AWS SigV4 auth configuration for this provider.
    pub aws: Option<ModelProviderAwsAuthInfo>,
    /// Which wire protocol this provider expects.
    #[serde(default)]
    pub wire_api: WireApi,
    /// Optional API capability overrides for a custom Responses-compatible provider.
    /// Unspecified capabilities retain their existing provider defaults.
    pub capabilities: Option<ModelProviderCapabilities>,
    /// Optional query parameters to append to the base URL.
    pub query_params: Option<HashMap<String, RedactedString>>,
    /// Additional HTTP headers to include in requests to this provider where
    /// the (key, value) pairs are the header name and value.
    pub http_headers: Option<HashMap<String, RedactedString>>,
    /// Optional HTTP headers to include in requests to this provider where the
    /// (key, value) pairs are the header name and _environment variable_ whose
    /// value should be used. If the environment variable is not set, or the
    /// value is empty, the header will not be included in the request.
    pub env_http_headers: Option<HashMap<String, String>>,
    /// Maximum number of times to retry a failed HTTP request to this provider.
    pub request_max_retries: Option<u64>,
    /// Number of times to retry reconnecting a dropped streaming response before failing.
    pub stream_max_retries: Option<u64>,
    /// Idle timeout (in milliseconds) to wait for activity on a streaming response before treating
    /// the connection as lost.
    pub stream_idle_timeout_ms: Option<u64>,
    /// Maximum time (in milliseconds) to wait for a websocket connection attempt before treating
    /// it as failed.
    pub websocket_connect_timeout_ms: Option<u64>,
    /// Does this provider require an Provider API key or ChatGPT login token? If true,
    /// the user is presented with a login screen on first run, and credentials are
    /// stored using the backend selected by `cli_auth_credentials_store`.
    /// If false (the default), the login screen is skipped, and the API key
    /// (if needed) comes from the environment variable specified by `env_key`.
    #[serde(default)]
    pub requires_hosted_auth: bool,
    /// Whether this provider supports the Responses API WebSocket transport.
    #[serde(default)]
    pub supports_websockets: bool,
    /// Whether this provider supports the standalone web-search endpoint.
    #[serde(default)]
    pub supports_standalone_web_search: bool,
    /// Runtime-only opt-in for internal metadata, independent of the destination check.
    /// This cannot be loaded from or written to serialized provider configuration.
    #[serde(skip)]
    #[schemars(skip)]
    pub include_internal_metadata: bool,
}

/// AWS SigV4 auth configuration for a model provider.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ModelProviderAwsAuthInfo {
    /// AWS profile name to use. When unset, the AWS SDK default chain decides.
    pub profile: Option<String>,
    /// AWS region to use for provider-specific endpoints.
    pub region: Option<String>,
    /// Optional command whose exported credentials replace the AWS SDK credential chain.
    pub credential_export: Option<AwsCredentialExportConfig>,
    /// Optional command used to reauthenticate after a refreshable AWS auth failure.
    pub auth_refresh: Option<AwsAuthRefreshConfig>,
}

/// Command used to export AWS signing credentials for a model provider.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AwsCredentialExportConfig {
    /// Executable to invoke directly, without a shell.
    pub command: String,
    /// Arguments passed to the credential export command.
    #[serde(default)]
    pub args: Vec<RedactedString>,
    /// Maximum time to wait for the credential export command to complete.
    #[serde(default = "default_aws_credential_export_timeout_ms")]
    pub timeout_ms: NonZeroU64,
}

impl AwsCredentialExportConfig {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.get())
    }
}

fn default_aws_credential_export_timeout_ms() -> NonZeroU64 {
    match NonZeroU64::new(DEFAULT_AWS_CREDENTIAL_EXPORT_TIMEOUT_MS) {
        Some(timeout_ms) => timeout_ms,
        None => panic!("AWS credential export timeout must be non-zero"),
    }
}

/// Command used to refresh AWS credentials for a model provider.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AwsAuthRefreshConfig {
    /// Executable to invoke directly, without a shell.
    pub command: String,
    /// Arguments passed to the refresh command.
    #[serde(default)]
    pub args: Vec<RedactedString>,
    /// Maximum time to wait for the refresh command to complete.
    #[serde(default = "default_aws_auth_refresh_timeout_ms")]
    pub timeout_ms: NonZeroU64,
}

impl AwsAuthRefreshConfig {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.get())
    }
}

fn default_aws_auth_refresh_timeout_ms() -> NonZeroU64 {
    match NonZeroU64::new(DEFAULT_AWS_AUTH_REFRESH_TIMEOUT_MS) {
        Some(timeout_ms) => timeout_ms,
        None => panic!("AWS auth refresh timeout must be non-zero"),
    }
}

impl ModelProviderInfo {
    /// Validate the effective provider before creating any runtime clients.
    pub fn validate_custom_provider(&self) -> Result<(), String> {
        if self.requires_hosted_auth {
            return Err(CUSTOM_PROVIDER_SETUP.to_string());
        }
        let base_url = self
            .base_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .ok_or_else(|| CUSTOM_PROVIDER_SETUP.to_string())?;
        Self::validate_custom_url(base_url)?;
        if let Some(catalog_url) = &self.model_catalog_url {
            Self::validate_custom_url(catalog_url)?;
        }
        self.validate()
    }

    fn validate_custom_url(value: &str) -> Result<(), String> {
        let url = url::Url::parse(value)
            .map_err(|_| "custom provider URL must be an absolute HTTP(S) URL".to_string())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "custom provider URL must be HTTP(S), without embedded credentials or a fragment"
                    .to_string(),
            );
        }
        xdcode_client::ensure_custom_service_url(&url)
            .map_err(|_| "official service endpoints are disabled in xdcode; configure your own Responses provider".to_string())
    }

    /// Checks that a configured Bedrock entry only customizes supported fields.
    /// Call this on the override before merging it with the built-in provider.
    pub fn validate_bedrock_override(&self) -> Result<(), String> {
        let unsupported_fields = Self {
            base_url: None,
            auth: None,
            aws: None,
            http_headers: None,
            ..self.clone()
        };
        if unsupported_fields != Self::default() {
            return Err("only supports changing \
`base_url`, `auth`, `http_headers`, `aws.profile`, `aws.region`, `aws.credential_export`, \
and `aws.auth_refresh`; \
other non-default provider fields are not supported"
                .to_string());
        }
        Ok(())
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if let Some(gateway) = &self.gateway_oauth {
            gateway.validate(self)?;
        }
        if let Some(aws) = self.aws.as_ref() {
            if self.supports_websockets {
                // TODO(celia-oai): Support AWS SigV4 signing for WebSocket
                // upgrade requests before allowing AWS-authenticated providers
                // to enable Responses-over-WebSocket.
                return Err("provider aws cannot be combined with supports_websockets".to_string());
            }

            let mut conflicts = Vec::new();
            if self.env_key.is_some() {
                conflicts.push("env_key");
            }
            if self.experimental_bearer_token.is_some() {
                conflicts.push("experimental_bearer_token");
            }
            if self.auth.is_some() {
                conflicts.push("auth");
            }
            if self.requires_hosted_auth {
                conflicts.push("requires_hosted_auth");
            }

            if !conflicts.is_empty() {
                return Err(format!(
                    "provider aws cannot be combined with {}",
                    conflicts.join(", ")
                ));
            }

            if let Some(credential_export) = aws.credential_export.as_ref() {
                if aws.profile.is_some() {
                    return Err(
                        "provider aws.credential_export cannot be combined with aws.profile"
                            .to_string(),
                    );
                }
                if credential_export.command.trim().is_empty() {
                    return Err(
                        "provider aws.credential_export.command must not be empty".to_string()
                    );
                }
                let command = Path::new(&credential_export.command);
                let mut components = command.components();
                let is_bare_command = matches!(
                    (components.next(), components.next()),
                    (Some(Component::Normal(name)), None) if name == command.as_os_str()
                );
                if !command.is_absolute() && !is_bare_command {
                    return Err(
                        "provider aws.credential_export.command must be an absolute path or a bare executable name"
                            .to_string(),
                    );
                }
            }

            if let Some(auth_refresh) = aws.auth_refresh.as_ref() {
                if auth_refresh.command.trim().is_empty() {
                    return Err("provider aws.auth_refresh.command must not be empty".to_string());
                }
                if auth_refresh.command != "aws" {
                    return Err("provider aws.auth_refresh.command must be `aws`".to_string());
                }
            }
        }

        let Some(auth) = self.auth.as_ref() else {
            return Ok(());
        };

        if auth.command.trim().is_empty() {
            return Err("provider auth.command must not be empty".to_string());
        }

        let mut conflicts = Vec::new();
        if self.env_key.is_some() {
            conflicts.push("env_key");
        }
        if self.experimental_bearer_token.is_some() {
            conflicts.push("experimental_bearer_token");
        }
        if self.requires_hosted_auth {
            conflicts.push("requires_hosted_auth");
        }

        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "provider auth cannot be combined with {}",
                conflicts.join(", ")
            ))
        }
    }

    fn build_header_map(&self) -> XdcodeResult<HeaderMap> {
        let capacity = self.http_headers.as_ref().map_or(0, HashMap::len)
            + self.env_http_headers.as_ref().map_or(0, HashMap::len);
        let mut headers = HeaderMap::with_capacity(capacity);
        if let Some(extra) = &self.http_headers {
            for (k, v) in extra {
                if let (Ok(name), Ok(value)) =
                    (HeaderName::try_from(k), HeaderValue::try_from(v.as_str()))
                {
                    headers.insert(name, value);
                }
            }
        }

        if let Some(env_headers) = &self.env_http_headers {
            for (header, env_var) in env_headers {
                if let Ok(val) = std::env::var(env_var)
                    && !val.trim().is_empty()
                    && let (Ok(name), Ok(value)) =
                        (HeaderName::try_from(header), HeaderValue::try_from(val))
                {
                    headers.insert(name, value);
                }
            }
        }

        Ok(headers)
    }

    /// Builds an API provider with managed residency taking precedence over configured headers.
    pub fn to_api_provider(&self, auth_mode: Option<AuthMode>) -> XdcodeResult<ApiProvider> {
        if auth_mode.is_some_and(AuthMode::uses_xdcode_backend) {
            return Err(XdcodeErr::InvalidRequest(CUSTOM_PROVIDER_SETUP.to_string()));
        }
        self.validate_custom_provider()
            .map_err(XdcodeErr::InvalidRequest)?;
        let base_url = self
            .base_url
            .clone()
            .ok_or_else(|| XdcodeErr::InvalidRequest(CUSTOM_PROVIDER_SETUP.to_string()))?;

        let mut headers = self.build_header_map()?;
        if let Some(requirement) = read_managed_residency_requirement() {
            let value = match requirement {
                ResidencyRequirement::Us => HeaderValue::from_static("us"),
            };
            headers.insert(RESIDENCY_HEADER_NAME, value);
        }
        let retry = ApiRetryConfig {
            max_attempts: self.request_max_retries(),
            base_delay: Duration::from_millis(200),
            retry_429: false,
            retry_5xx: true,
            retry_transport: true,
        };

        Ok(ApiProvider {
            name: self.name.clone(),
            base_url,
            query_params: self.query_params.clone().map(|params| {
                params
                    .into_iter()
                    .map(|(name, value)| (name, value.into_inner()))
                    .collect()
            }),
            headers,
            retry,
            stream_idle_timeout: self.stream_idle_timeout(),
        })
    }

    /// If `env_key` is Some, returns the API key for this provider if present
    /// (and non-empty) in the environment. If `env_key` is required but
    /// cannot be found, returns an error.
    pub fn api_key(&self) -> XdcodeResult<Option<String>> {
        match &self.env_key {
            Some(env_key) => {
                let api_key = std::env::var(env_key)
                    .ok()
                    .filter(|v| !v.trim().is_empty())
                    .ok_or_else(|| {
                        XdcodeErr::EnvVar(EnvVarError {
                            var: env_key.clone(),
                            instructions: self.env_key_instructions.clone(),
                        })
                    })?;
                Ok(Some(api_key))
            }
            None => Ok(None),
        }
    }

    /// Effective maximum number of request retries for this provider.
    pub fn request_max_retries(&self) -> u64 {
        self.request_max_retries
            .unwrap_or(DEFAULT_REQUEST_MAX_RETRIES)
            .min(MAX_REQUEST_MAX_RETRIES)
    }

    /// Effective maximum number of stream reconnection attempts for this provider.
    pub fn stream_max_retries(&self) -> u64 {
        self.stream_max_retries
            .unwrap_or(DEFAULT_STREAM_MAX_RETRIES)
            .min(MAX_STREAM_MAX_RETRIES)
    }

    /// Effective idle timeout for streaming responses.
    pub fn stream_idle_timeout(&self) -> Duration {
        self.stream_idle_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(DEFAULT_STREAM_IDLE_TIMEOUT_MS))
    }

    /// Effective timeout for websocket connect attempts.
    pub fn websocket_connect_timeout(&self) -> Duration {
        self.websocket_connect_timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(DEFAULT_WEBSOCKET_CONNECT_TIMEOUT_MS))
    }

    /// Retained constructor name for callers; creates a custom Responses provider.
    /// An absent URL is a setup placeholder, never an official endpoint fallback.
    pub fn create_custom_provider(base_url: Option<String>) -> ModelProviderInfo {
        ModelProviderInfo {
            name: "Custom Responses".into(),
            base_url,
            model_catalog_url: None,
            env_key: Some("XDCODE_API_KEY".into()),
            env_key_instructions: Some(
                "Export XDCODE_API_KEY for your configured Responses provider.".into(),
            ),
            experimental_bearer_token: None,
            auth: None,
            gateway_oauth: None,
            aws: None,
            wire_api: WireApi::Responses,
            query_params: None,
            http_headers: Some(
                [("version".to_string(), env!("CARGO_PKG_VERSION").into())]
                    .into_iter()
                    .collect(),
            ),
            env_http_headers: None,
            // Use global defaults for retry/timeout unless overridden in config.toml.
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_hosted_auth: false,
            supports_websockets: false,
            supports_standalone_web_search: false,
            capabilities: None,
            include_internal_metadata: false,
        }
    }

    pub fn create_amazon_bedrock_provider(
        aws: Option<ModelProviderAwsAuthInfo>,
    ) -> ModelProviderInfo {
        ModelProviderInfo {
            name: AMAZON_BEDROCK_PROVIDER_NAME.into(),
            // The runtime provider derives the regional Mantle endpoint when
            // this is unset. A configured value is therefore unambiguously an
            // endpoint override.
            base_url: None,
            model_catalog_url: None,
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            gateway_oauth: None,
            aws: Some(aws.unwrap_or(ModelProviderAwsAuthInfo {
                profile: None,
                region: None,
                credential_export: None,
                auth_refresh: None,
            })),
            wire_api: WireApi::Responses,
            query_params: None,
            http_headers: Some(HashMap::from([(
                AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_HEADER.to_string(),
                AMAZON_BEDROCK_MANTLE_CLIENT_AGENT_VALUE.into(),
            )])),
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_hosted_auth: false,
            supports_websockets: false,
            supports_standalone_web_search: false,
            capabilities: None,
            include_internal_metadata: false,
        }
    }

    pub fn create_amazon_bedrock_runtime_provider(
        aws: Option<ModelProviderAwsAuthInfo>,
    ) -> ModelProviderInfo {
        let mut provider = Self::create_amazon_bedrock_provider(aws);
        provider.name = AMAZON_BEDROCK_RUNTIME_PROVIDER_NAME.into();
        provider
    }

    pub fn create_gcp_provider() -> ModelProviderInfo {
        ModelProviderInfo {
            name: GCP_PROVIDER_NAME.into(),
            base_url: None,
            model_catalog_url: None,
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            // Config loading supplies default command auth after resolving its working directory.
            auth: None,
            gateway_oauth: None,
            aws: None,
            capabilities: None,
            wire_api: WireApi::Responses,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_hosted_auth: false,
            supports_websockets: false,
            supports_standalone_web_search: false,
            include_internal_metadata: false,
        }
    }

    pub fn is_hosted_provider(&self) -> bool {
        false
    }

    pub fn supports_xdcode_backend_routes(&self) -> bool {
        false
    }

    pub fn uses_provider_actor_authorization(&self) -> bool {
        !self.requires_hosted_auth
            && self.http_headers.as_ref().is_some_and(|headers| {
                headers.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case(PROVIDER_ACTOR_AUTHORIZATION_HEADER)
                        && !value.trim().is_empty()
                })
            })
    }

    pub fn is_amazon_bedrock(&self) -> bool {
        self.name == AMAZON_BEDROCK_PROVIDER_NAME
            || self.name == AMAZON_BEDROCK_RUNTIME_PROVIDER_NAME
    }

    pub fn is_amazon_bedrock_runtime(&self) -> bool {
        self.name == AMAZON_BEDROCK_RUNTIME_PROVIDER_NAME
    }

    pub fn is_gcp(&self) -> bool {
        self.name == GCP_PROVIDER_NAME
    }

    pub fn has_command_auth(&self) -> bool {
        self.auth.is_some()
    }
}

pub const DEFAULT_LMSTUDIO_PORT: u16 = 1234;
pub const DEFAULT_OLLAMA_PORT: u16 = 11434;

pub const LMSTUDIO_OSS_PROVIDER_ID: &str = "lmstudio";
pub const OLLAMA_OSS_PROVIDER_ID: &str = "ollama";

/// Endpoint-less setup placeholder. Every usable provider comes from user configuration.
pub fn built_in_model_providers(
    _provider_base_url: Option<String>,
) -> HashMap<String, ModelProviderInfo> {
    // A setup placeholder has no endpoint and cannot send requests.
    HashMap::from([(
        CUSTOM_PROVIDER_ID.to_string(),
        ModelProviderInfo::create_custom_provider(None),
    )])
}

/// Merge configured providers into the built-in provider catalog.
///
/// Configured providers replace the setup placeholder and supply their own URLs.
pub fn merge_configured_model_providers(
    mut model_providers: HashMap<String, ModelProviderInfo>,
    configured_model_providers: HashMap<String, ModelProviderInfo>,
) -> Result<HashMap<String, ModelProviderInfo>, String> {
    for (key, mut provider) in configured_model_providers {
        if provider.env_key.is_none()
            && provider.auth.is_none()
            && provider.gateway_oauth.is_none()
            && provider.aws.is_none()
            && provider.experimental_bearer_token.is_none()
        {
            provider.env_key = Some("XDCODE_API_KEY".to_string());
        }
        model_providers.insert(key, provider);
    }

    Ok(model_providers)
}

pub fn create_oss_provider(default_provider_port: u16, wire_api: WireApi) -> ModelProviderInfo {
    // These XDCODE_OSS_ environment variables are experimental: we may
    // switch to reading values from config.toml instead.
    let default_xdcode_oss_base_url = format!(
        "http://localhost:{xdcode_oss_port}/v1",
        xdcode_oss_port = std::env::var("XDCODE_OSS_PORT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_provider_port)
    );

    let xdcode_oss_base_url = std::env::var("XDCODE_OSS_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(default_xdcode_oss_base_url);
    create_oss_provider_with_base_url(&xdcode_oss_base_url, wire_api)
}

pub fn create_oss_provider_with_base_url(base_url: &str, wire_api: WireApi) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "gpt-oss".into(),
        base_url: Some(base_url.into()),
        model_catalog_url: None,
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        gateway_oauth: None,
        aws: None,
        wire_api,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_hosted_auth: false,
        supports_websockets: false,
        supports_standalone_web_search: false,
        capabilities: None,
        include_internal_metadata: false,
    }
}

#[cfg(test)]
#[path = "model_provider_info_tests.rs"]
mod tests;

#[cfg(test)]
mod custom_runtime_tests {
    use super::*;

    #[test]
    fn missing_endpoint_explains_custom_setup_for_all_auth_modes() {
        let provider = ModelProviderInfo::create_custom_provider(None);
        for auth in [None, Some(AuthMode::ApiKey), Some(AuthMode::Chatgpt)] {
            let error = provider.to_api_provider(auth).unwrap_err().to_string();
            assert!(error.contains("XDCODE_API_KEY"));
            assert!(error.contains("base_url"));
        }
        let providers = built_in_model_providers(Some("https://api.openai.com/v1".into()));
        assert_eq!(providers.len(), 1);
        assert!(providers[CUSTOM_PROVIDER_ID].base_url.is_none());
    }

    #[test]
    fn custom_endpoint_is_preserved_and_uses_xdcode_key() {
        let providers = merge_configured_model_providers(
            built_in_model_providers(None),
            HashMap::from([(
                "custom".into(),
                ModelProviderInfo {
                    name: "Personal provider".into(),
                    base_url: Some("https://provider.example/v1".into()),
                    ..ModelProviderInfo::default()
                },
            )]),
        )
        .unwrap();
        let provider = &providers[CUSTOM_PROVIDER_ID];
        assert_eq!(provider.env_key.as_deref(), Some("XDCODE_API_KEY"));
        assert_eq!(
            provider.to_api_provider(None).unwrap().base_url,
            "https://provider.example/v1"
        );
        assert!(!provider.supports_xdcode_backend_routes());
        assert!(provider.to_api_provider(Some(AuthMode::Chatgpt)).is_err());
    }

    #[test]
    fn official_or_missing_catalog_destinations_cannot_be_used() {
        for base_url in [
            "https://api.openai.com/v1",
            "https://CHATGPT.COM./backend-api/codex",
            "file:///tmp/provider",
            "https://user:password@provider.example/v1",
        ] {
            let provider = ModelProviderInfo::create_custom_provider(Some(base_url.into()));
            assert!(provider.validate_custom_provider().is_err());
        }
        let mut provider =
            ModelProviderInfo::create_custom_provider(Some("http://localhost:8080/v1".into()));
        assert!(provider.validate_custom_provider().is_ok());
        provider.model_catalog_url = Some("https://chatgpt.com/backend-api/codex/models".into());
        assert!(provider.validate_custom_provider().is_err());
    }
}
