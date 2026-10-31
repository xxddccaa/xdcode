//! Bounded Responses-only capability discovery. Requests never contain project data,
//! conversation history or tools, and request failures are never negative capabilities.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use futures::StreamExt;
use reqwest::StatusCode;
use serde_json::Value;
use serde_json::json;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_protocol::model_types::ReasoningEffort;

use super::CatalogModel;
use super::CatalogSnapshot;
use super::DiscoveryRefresh;
use super::DiscoverySource;
use super::ModelCheck;
use super::ModelCompatibility;
use super::ModelStatus;
use super::ReasoningDiscovery;
use super::edit_catalog;
use super::efforts_fingerprint;
use super::interface_fingerprint;
use super::now_unix;
use super::set_model;
use super::static_check;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_CANDIDATES: usize = 16;

struct ProbeClient {
    client: reqwest::Client,
    provider: ModelProviderInfo,
    headers: reqwest::header::HeaderMap,
    base_url: reqwest::Url,
}

impl ProbeClient {
    fn new(provider: &ModelProviderInfo) -> Result<Self> {
        if provider.auth.is_some() || provider.gateway_oauth.is_some() || provider.aws.is_some() {
            bail!(
                "capability probing requires an environment or existing bearer credential; command/OAuth/AWS authentication must be verified through the normal runtime client"
            );
        }
        let api_provider = provider
            .to_api_provider(None)
            .map_err(|_| anyhow::anyhow!("invalid Responses provider configuration"))?;
        let mut headers = api_provider.headers;
        if let Some(token) = provider
            .api_key()
            .map_err(|_| anyhow::anyhow!("provider credential environment variable is missing"))?
            .or_else(|| {
                provider
                    .experimental_bearer_token
                    .clone()
                    .map(|s| s.into_inner())
            })
        {
            let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .context("invalid bearer credential")?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        let base_url =
            reqwest::Url::parse(&api_provider.base_url).context("invalid provider endpoint")?;
        if !matches!(base_url.scheme(), "http" | "https") {
            bail!("provider must use HTTP or HTTPS");
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            provider: provider.clone(),
            headers,
            base_url,
        })
    }

    fn endpoint(&self, segments: &[&str]) -> Result<reqwest::Url> {
        let mut url = self.base_url.clone();
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid provider base URL"))?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
        drop(path);
        Ok(url)
    }

    fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .request(method, url)
            .headers(self.headers.clone());
        if let Some(query) = &self.provider.query_params {
            let params: Vec<_> = query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect();
            request = request.query(&params);
        }
        request
    }

    async fn response(
        &self,
        model: &CatalogModel,
        effort: Option<&ReasoningEffort>,
    ) -> Result<ProbeResponse> {
        let mut body = json!({"model":model.model,"input":"Reply OK.","max_output_tokens":16,"stream":false,"store":false});
        if let Some(effort) = effort {
            body["reasoning"] = json!({"effort":effort.as_str()});
        }
        let response = self
            .request(reqwest::Method::POST, self.endpoint(&["responses"])?)
            .json(&body)
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "capability request failed (network or timeout); configuration was not changed"
                )
            })?;
        bounded_response(response).await
    }

    async fn metadata(&self, model: &CatalogModel) -> Result<Option<Vec<ReasoningEffort>>> {
        let url = if let Some(url) = &self.provider.model_catalog_url {
            reqwest::Url::parse(url.as_str()).context("invalid model catalog URL")?
        } else {
            self.endpoint(&["models", &model.model])?
        };
        // Never send inference credentials to a separately configured catalog origin.
        if url.origin() != self.base_url.origin() {
            return Ok(None);
        }
        let response = self
            .request(reqwest::Method::GET, url)
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("model metadata request failed; configuration was not changed")
            })?;
        let response = bounded_response(response).await?;
        if response.status.is_success() {
            if let Some(efforts) = metadata_efforts(&response.body, &model.model) {
                return Ok(Some(efforts));
            }
        } else if !matches!(response.status.as_u16(), 400 | 404 | 405 | 501) {
            fail_transient(response.status)?;
        }
        Ok(None)
    }
}

struct ProbeResponse {
    status: StatusCode,
    body: Value,
}

impl ProbeResponse {
    fn accepted(&self) -> bool {
        self.status.is_success()
            && self.body.get("object").and_then(Value::as_str) == Some("response")
            && self.body.get("id").and_then(Value::as_str).is_some()
            && self.body.get("output").is_some_and(Value::is_array)
            && matches!(
                self.body.get("status").and_then(Value::as_str),
                Some("completed" | "incomplete")
            )
            && self.body.get("error").is_none_or(Value::is_null)
    }
}

async fn bounded_response(response: reqwest::Response) -> Result<ProbeResponse> {
    let status = response.status();
    // Error text can contain credentials or upstream diagnostics; it is parsed locally and
    // never displayed verbatim. Stream and limit before allocating the entire body.
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow::anyhow!("capability response interrupted"))?;
        if bytes.len() + chunk.len() > MAX_BODY_BYTES {
            bail!("capability response exceeded the size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok(ProbeResponse { status, body })
}

fn fail_transient(status: StatusCode) -> Result<()> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        bail!("provider rejected credentials; no capability result was saved");
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        bail!("provider rate limited discovery; no capability result was saved");
    }
    if status.is_server_error() {
        bail!(
            "provider failed discovery (HTTP {}); no capability result was saved",
            status.as_u16()
        );
    }
    Ok(())
}

fn effort_values(value: &Value) -> Option<Vec<ReasoningEffort>> {
    let array = value.as_array()?;
    let mut seen = HashSet::new();
    let mut efforts = Vec::new();
    for item in array {
        let text = item.as_str().or_else(|| item.get("effort")?.as_str())?;
        if text.trim().is_empty() || text.len() > 128 || !seen.insert(text.to_owned()) {
            return None;
        }
        efforts.push(serde_json::from_value(json!(text)).ok()?);
    }
    Some(efforts)
}

fn metadata_efforts(value: &Value, model: &str) -> Option<Vec<ReasoningEffort>> {
    // A generic 'enum' or a message mentioning a value is not authoritative model metadata.
    if let Some(models) = value
        .get("models")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
    {
        return models
            .iter()
            .find(|entry| {
                entry
                    .get("id")
                    .or_else(|| entry.get("slug"))
                    .and_then(Value::as_str)
                    == Some(model)
            })
            .and_then(|entry| metadata_efforts(entry, model));
    }
    if let Some(id) = value
        .get("id")
        .or_else(|| value.get("slug"))
        .and_then(Value::as_str)
        && id != model
    {
        return None;
    }
    for field in [
        "reasoning_efforts",
        "supported_reasoning_efforts",
        "supported_reasoning_levels",
    ] {
        if let Some(values) = value.get(field).and_then(effort_values) {
            return Some(values);
        }
    }
    value
        .pointer("/capabilities/reasoning/efforts")
        .and_then(effort_values)
}

fn api_enum(value: &Value) -> Option<Vec<ReasoningEffort>> {
    let error = value.get("error")?;
    let param = error.get("param").and_then(Value::as_str)?;
    if !matches!(
        param,
        "reasoning.effort" | "reasoning_effort" | "reasoning/effort"
    ) {
        return None;
    }
    for key in ["allowed_values", "supported_values", "enum"] {
        if let Some(efforts) = error.get(key).and_then(effort_values) {
            return Some(efforts);
        }
    }
    // OpenAI-style errors can provide a machine-readable details object.
    error.pointer("/details/enum").and_then(effort_values)
}

/// Text returned by an API can suggest additional candidates, but cannot establish
/// an authoritative enumeration. Every value collected here still needs a probe.
fn suggested_efforts(value: &Value) -> Vec<ReasoningEffort> {
    let Some(error) = value.get("error") else {
        return Vec::new();
    };
    let param = error
        .get("param")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !(param.contains("reasoning") || message.to_ascii_lowercase().contains("reasoning"))
        || !["supported", "allowed", "valid values"]
            .iter()
            .any(|word| message.to_ascii_lowercase().contains(word))
    {
        return Vec::new();
    }
    let mut values = Vec::new();
    let mut quote = None;
    let mut token = String::new();
    for character in message.chars().take(4096) {
        if let Some(end) = quote {
            if character == end {
                if !token.is_empty()
                    && token.len() <= 128
                    && token.chars().all(|character| {
                        character.is_ascii_alphanumeric() || "_-./".contains(character)
                    })
                {
                    if let Ok(value) = serde_json::from_value::<ReasoningEffort>(json!(token)) {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                }
                quote = None;
            } else {
                token.push(character);
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            token.clear();
        }
        if values.len() >= MAX_CANDIDATES {
            break;
        }
    }
    values
}

async fn discover(
    provider: &ModelProviderInfo,
    model: &CatalogModel,
) -> Result<(Vec<ReasoningEffort>, DiscoverySource)> {
    let client = ProbeClient::new(provider)?;
    if let Some(efforts) = client.metadata(model).await? {
        return Ok((efforts, DiscoverySource::AuthoritativeMetadata));
    }
    let first_effort = ReasoningEffort::High;
    let first = client.response(model, Some(&first_effort)).await?;
    fail_transient(first.status)?;
    if let Some(efforts) = api_enum(&first.body) {
        return Ok((efforts, DiscoverySource::ApiEnum));
    }
    if matches!(first.status.as_u16(), 404 | 405 | 501) {
        bail!("provider does not expose a compatible Responses endpoint");
    }
    let mut verified = Vec::new();
    if first.accepted() {
        verified.push(first_effort.clone());
    }
    let candidates = [
        "none",
        "minimal",
        "low",
        "medium",
        "xhigh",
        "max",
        "ultra",
        "persistent",
    ];
    let common: Vec<ReasoningEffort> = candidates
        .iter()
        .filter_map(|value| serde_json::from_value(json!(value)).ok())
        .collect();
    let mut candidates = suggested_efforts(&first.body);
    if let Some(previous) = &model.reasoning_efforts {
        for effort in previous {
            if !candidates.contains(effort) && *effort != first_effort {
                candidates.push(effort.clone());
            }
        }
    }
    for effort in common {
        if !candidates.contains(&effort) && effort != first_effort {
            candidates.push(effort);
        }
    }
    candidates.truncate(MAX_CANDIDATES - 1);
    let mut probes = futures::stream::iter(candidates.into_iter().map(|effort| {
        let client = &client;
        async move {
            let response = client.response(model, Some(&effort)).await?;
            Ok::<_, anyhow::Error>((effort, response))
        }
    }))
    .buffer_unordered(2);
    while let Some(result) = probes.next().await {
        let (effort, response) = result?;
        fail_transient(response.status)?;
        if let Some(efforts) = api_enum(&response.body) {
            return Ok((efforts, DiscoverySource::ApiEnum));
        }
        if response.accepted() {
            verified.push(effort);
        }
    }
    if verified.is_empty() {
        // An empty candidate result does not establish that reasoning is unsupported.
        bail!(
            "no reasoning levels could be confirmed; service-default support and capability completeness remain unknown"
        );
    }
    verified.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    Ok((verified, DiscoverySource::VerifiedProbes))
}

/// Explicit compatibility check. Static checks never enter this function implicitly.
pub async fn probe_model(snapshot: &CatalogSnapshot, id: &str) -> Result<ModelCheck> {
    let mut check = static_check(snapshot, id)?;
    if check.status == ModelStatus::MissingCredentials || check.status == ModelStatus::Incompatible
    {
        return Ok(check);
    }
    let model = snapshot.model(id)?;
    let provider = snapshot
        .providers
        .get(&model.provider)
        .context("missing provider")?;
    let client = ProbeClient::new(provider)?;
    let response = tokio::time::timeout(DISCOVERY_TIMEOUT, client.response(model, None))
        .await
        .context("capability check exceeded 60 seconds")??;
    fail_transient(response.status)?;
    if response.accepted() {
        check.status = ModelStatus::Ready;
        check.issues.push("Responses request accepted; tool calling and reasoning levels remain independently unverified".into());
    } else {
        check.status = ModelStatus::Incompatible;
        check.issues.push(format!(
            "Responses request rejected (HTTP {}); no automatic replay or model substitution",
            response.status.as_u16()
        ));
    }
    Ok(check)
}

/// Missing lists are discovered once. Manual lists (including []) never cause a request
/// unless the caller explicitly opts into replacing user-maintained data.
pub async fn refresh_reasoning(
    path: &Path,
    id: &str,
    overwrite_manual: bool,
) -> Result<DiscoveryRefresh> {
    let initial = CatalogSnapshot::load(path)?;
    refresh_with_snapshot(path, id, overwrite_manual, initial, None).await
}

/// Runtime entry point: discover against the effective layered provider definition,
/// while persisting only in the model directory's writable defining file.
pub async fn refresh_reasoning_layered(
    stack: &crate::ConfigLayerStack,
    id: &str,
    overwrite_manual: bool,
) -> Result<DiscoveryRefresh> {
    let path = super::catalog_source(stack)?;
    let initial = CatalogSnapshot::load_layered(stack)?;
    refresh_with_snapshot(&path, id, overwrite_manual, initial, Some(stack)).await
}

async fn refresh_with_snapshot(
    path: &Path,
    id: &str,
    overwrite_manual: bool,
    initial: CatalogSnapshot,
    stack: Option<&crate::ConfigLayerStack>,
) -> Result<DiscoveryRefresh> {
    let initial_model = initial.model(id)?.clone();
    if initial_model.is_user_maintained() && !overwrite_manual {
        return Ok(DiscoveryRefresh {
            model: initial_model,
            saved: true,
            message: Some(
                "user-maintained reasoning levels retained; explicitly opt in to replacing them"
                    .into(),
            ),
        });
    }
    let check = static_check(&initial, id)?;
    if check.status == ModelStatus::MissingCredentials {
        bail!(
            "missing credentials: {}",
            check.missing_credentials.join(", ")
        );
    }
    if check.status == ModelStatus::Incompatible {
        bail!("model is incompatible: {}", check.issues.join("; "));
    }
    let provider = initial
        .providers
        .get(&initial_model.provider)
        .context("missing provider")?;
    let (efforts, source) =
        tokio::time::timeout(DISCOVERY_TIMEOUT, discover(provider, &initial_model))
            .await
            .context("reasoning discovery exceeded 60 seconds; configuration was not changed")??;
    let mut discovered = initial_model.clone();
    discovered.reasoning_discovery = Some(ReasoningDiscovery {
        complete: source != DiscoverySource::VerifiedProbes,
        source,
        discovered_at: now_unix(),
        model: discovered.model.clone(),
        interface_fingerprint: interface_fingerprint(provider),
        efforts_fingerprint: efforts_fingerprint(&efforts),
    });
    discovered.reasoning_efforts = Some(efforts);
    if discovered
        .default_reasoning_effort
        .as_ref()
        .is_some_and(|effort| !discovered.supports_effort(effort))
    {
        discovered.default_reasoning_effort = None;
    }
    // Preserve compatibility until a real Responses request has confirmed it. Metadata alone
    // says nothing about transport/tool compatibility.
    if discovered
        .reasoning_discovery
        .as_ref()
        .is_some_and(|record| record.source == DiscoverySource::VerifiedProbes)
    {
        discovered.compatibility = ModelCompatibility::Responses;
    }
    let save = (|| -> Result<()> {
        // Capture the defining file version before the layered read. The writer's
        // compare-and-swap guard then covers changes throughout the merge window.
        let file_version = CatalogSnapshot::load(path)?.version;
        let latest = match stack {
            Some(stack) => CatalogSnapshot::load_layered(stack)?,
            None => CatalogSnapshot::load(path)?,
        };
        if latest.model(id)? != &initial_model
            || latest
                .providers
                .get(&initial_model.provider)
                .map(interface_fingerprint)
                != Some(interface_fingerprint(provider))
        {
            bail!("model or provider changed during discovery; new configuration was preserved");
        }
        edit_catalog(path, Some(&file_version), false, |doc| {
            set_model(doc, &discovered)
        })?;
        Ok(())
    })();
    match save {
        Ok(_) => Ok(DiscoveryRefresh {
            model: discovered,
            saved: true,
            message: None,
        }),
        Err(error) => Ok(DiscoveryRefresh {
            model: discovered,
            saved: false,
            message: Some(format!(
                "discovery succeeded but could not be saved: {error}"
            )),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    async fn mock_server(
        body: Value,
        status: u16,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let requests = received.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let requests = requests.clone();
                let body = body.to_string();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buffer = [0; 4096];
                    loop {
                        let length = socket.read(&mut buffer).await.unwrap();
                        if length == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buffer[..length]);
                        if let Some(offset) = raw.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&raw[..offset]).to_lowercase();
                            let content_length: usize = headers
                                .lines()
                                .find_map(|line| {
                                    line.strip_prefix("content-length:")
                                        .and_then(|s| s.trim().parse().ok())
                                })
                                .unwrap_or(0);
                            if raw.len() >= offset + 4 + content_length {
                                break;
                            }
                        }
                    }
                    requests.lock().await.push(String::from_utf8(raw).unwrap());
                    let response = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        (format!("http://{address}/v1"), received, task)
    }

    fn completed() -> Value {
        json!({"id":"resp-test","object":"response","status":"completed","output":[],"error":null})
    }

    fn test_model(id: &str) -> CatalogModel {
        CatalogModel {
            id: id.into(),
            name: id.into(),
            provider: id.into(),
            model: format!("{id}-model"),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn two_mock_responses_endpoints_isolate_auth_model_effort_and_history() {
        let (url_one, requests_one, task_one) = mock_server(completed(), 200).await;
        let (url_two, requests_two, task_two) = mock_server(completed(), 200).await;
        let one = ModelProviderInfo {
            name: "one".into(),
            base_url: Some(url_one),
            experimental_bearer_token: Some("test-key-one".into()),
            ..Default::default()
        };
        let two = ModelProviderInfo {
            name: "two".into(),
            base_url: Some(url_two),
            experimental_bearer_token: Some("test-key-two".into()),
            ..Default::default()
        };
        let first = ProbeClient::new(&one)
            .unwrap()
            .response(
                &test_model("one"),
                Some(&ReasoningEffort::Custom("vendor-deep".into())),
            )
            .await
            .unwrap();
        let second = ProbeClient::new(&two)
            .unwrap()
            .response(&test_model("two"), None)
            .await
            .unwrap();
        assert!(first.accepted() && second.accepted());
        let first = requests_one.lock().await[0].clone();
        let second = requests_two.lock().await[0].clone();
        assert!(
            first
                .to_lowercase()
                .contains("authorization: bearer test-key-one")
        );
        assert!(!first.contains("test-key-two"));
        assert!(
            second
                .to_lowercase()
                .contains("authorization: bearer test-key-two")
        );
        assert!(!second.contains("test-key-one"));
        let body_one: Value =
            serde_json::from_str(first.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        let body_two: Value =
            serde_json::from_str(second.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body_one["model"], "one-model");
        assert_eq!(body_one["reasoning"]["effort"], "vendor-deep");
        assert_eq!(body_two["model"], "two-model");
        assert!(body_two.get("reasoning").is_none());
        assert_eq!(body_one["input"], "Reply OK.");
        assert!(body_one.get("tools").is_none());
        assert!(body_one.get("previous_response_id").is_none());
        assert_eq!(body_one["max_output_tokens"], 16);
        task_one.abort();
        task_two.abort();
    }

    #[tokio::test]
    async fn read_only_file_retains_metadata_discovery_result_for_this_run() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let (url, _, server) = mock_server(
                json!({"id":"one-model","reasoning_efforts":["low","vendor-deep"]}),
                200,
            )
            .await;
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("config.toml");
            let mut model = test_model("one");
            model.reasoning_efforts = None;
            let provider = ModelProviderInfo {
                name: "one".into(),
                base_url: Some(url),
                experimental_bearer_token: Some("test-key".into()),
                ..Default::default()
            };
            let document = format!(
                "{}\n[model_providers.one]\n{}",
                {
                    let model_table = toml::to_string(&model).unwrap();
                    format!("[[models]]\n{model_table}")
                },
                toml::to_string(&provider).unwrap()
            );
            std::fs::write(&path, &document).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
            let result = refresh_reasoning(&path, "one", false).await.unwrap();
            assert!(!result.saved);
            assert_eq!(
                result.model.reasoning_efforts.unwrap()[1].as_str(),
                "vendor-deep"
            );
            assert!(result.model.reasoning_discovery.unwrap().complete);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), document);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            server.abort();
        }
    }

    #[test]
    fn arbitrary_success_payloads_are_not_responses_compatibility() {
        assert!(
            !ProbeResponse {
                status: StatusCode::OK,
                body: json!({})
            }
            .accepted()
        );
        assert!(
            !ProbeResponse {
                status: StatusCode::OK,
                body: json!({"error":{"message":"failure"}})
            }
            .accepted()
        );
        assert!(
            ProbeResponse {
                status: StatusCode::OK,
                body: completed()
            }
            .accepted()
        );
    }

    #[test]
    fn text_enum_suggestions_are_probe_candidates_and_never_complete() {
        let error = json!({"error":{"param":"reasoning.effort","message":"Unsupported reasoning effort. Supported values: 'low', 'vendor-deep'."}});
        assert!(api_enum(&error).is_none());
        let candidates = suggested_efforts(&error);
        assert!(
            candidates
                .iter()
                .any(|effort| effort.as_str() == "vendor-deep")
        );
    }

    #[test]
    fn explicit_error_enumerations_support_vendor_values() {
        let values =
            api_enum(&json!({"error":{"param":"reasoning.effort","enum":["low","vendor-deep"]}}))
                .unwrap();
        assert_eq!(values[1].as_str(), "vendor-deep");
        assert!(api_enum(&json!({"error":{"message":"supported: low, high"}})).is_none());
        assert!(api_enum(&json!({"error":{"param":"model","enum":["low"]}})).is_none());
    }

    #[test]
    fn metadata_is_bound_to_the_requested_model() {
        assert!(
            metadata_efforts(
                &json!({"id":"other","reasoning_efforts":["high"]}),
                "target"
            )
            .is_none()
        );
        assert_eq!(
            metadata_efforts(
                &json!({"data":[{"id":"target","reasoning_efforts":[]}]}),
                "target"
            ),
            Some(Vec::new())
        );
    }

    #[test]
    fn authentication_and_rate_limits_are_errors_not_empty_capabilities() {
        assert!(fail_transient(StatusCode::UNAUTHORIZED).is_err());
        assert!(fail_transient(StatusCode::TOO_MANY_REQUESTS).is_err());
        assert!(fail_transient(StatusCode::INTERNAL_SERVER_ERROR).is_err());
    }
}
