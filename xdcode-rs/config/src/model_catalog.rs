//! User-owned model directory and the single persistence/discovery API used by the CLI and TUI.
//!
//! `None`, `Some([])` and `Some(["none"])` are deliberately different: discover,
//! service default (omit reasoning.effort), and the literal provider value `none`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;
use xdcode_model_provider_info::ModelProviderInfo;
use xdcode_protocol::model_types::ReasoningEffort;
use xdcode_utils_path::resolve_symlink_write_paths;
use xdcode_utils_path::write_atomically;

mod discovery;
pub use discovery::probe_model;
pub use discovery::refresh_reasoning;
pub use discovery::refresh_reasoning_layered;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelCompatibility {
    #[default]
    Unverified,
    Responses,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    AuthoritativeMetadata,
    ApiEnum,
    VerifiedProbes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ReasoningDiscovery {
    pub source: DiscoverySource,
    /// Unix seconds; no inference about freshness is made from age alone.
    pub discovered_at: u64,
    pub model: String,
    /// Hash of provider definition, including endpoint, headers and authentication routing.
    pub interface_fingerprint: String,
    /// Only an explicit authoritative enumeration can be complete.
    pub complete: bool,
    /// Detects a user's edits to an automatically populated list.
    pub efforts_fingerprint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_efforts: Option<Vec<ReasoningEffort>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<i64>,
    #[serde(default)]
    pub compatibility: ModelCompatibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_discovery: Option<ReasoningDiscovery>,
}

impl CatalogModel {
    pub fn supports_effort(&self, effort: &ReasoningEffort) -> bool {
        self.reasoning_efforts
            .as_ref()
            .is_some_and(|values| values.contains(effort))
    }

    pub fn is_user_maintained(&self) -> bool {
        self.reasoning_efforts.is_some()
            && self.reasoning_discovery.as_ref().is_none_or(|record| {
                efforts_fingerprint(self.reasoning_efforts.as_deref().unwrap_or_default())
                    != record.efforts_fingerprint
            })
    }

    /// Clear stale generated results, and recognize hand-edited results as user maintained.
    pub fn reconcile_discovery(&mut self, provider: &ModelProviderInfo) {
        let Some(record) = &self.reasoning_discovery else {
            return;
        };
        if self.is_user_maintained() {
            self.reasoning_discovery = None;
        } else if record.model != self.model
            || record.interface_fingerprint != interface_fingerprint(provider)
        {
            self.reasoning_efforts = None;
            self.reasoning_discovery = None;
        }
    }
}

pub fn deserialize_models<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<CatalogModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let models = Vec::<CatalogModel>::deserialize(deserializer)?;
    validate_models(&models).map_err(serde::de::Error::custom)?;
    Ok(models)
}

pub fn validate_models(models: &[CatalogModel]) -> Result<()> {
    let mut ids = HashSet::new();
    for entry in models {
        if entry.id.trim().is_empty() || !ids.insert(&entry.id) {
            bail!(
                "model directory IDs must be nonempty and unique: {:?}",
                entry.id
            );
        }
        if entry.name.trim().is_empty()
            || entry.provider.trim().is_empty()
            || entry.model.trim().is_empty()
        {
            bail!("model {} requires name, provider and model", entry.id);
        }
        if entry.context_window.is_some_and(|n| n <= 0)
            || entry.auto_compact_token_limit.is_some_and(|n| n <= 0)
        {
            bail!("model {} context limits must be positive", entry.id);
        }
        if let Some(efforts) = &entry.reasoning_efforts {
            let mut seen = HashSet::new();
            for effort in efforts {
                if effort.as_str().trim().is_empty() || !seen.insert(effort.as_str()) {
                    bail!(
                        "model {} has empty or duplicate reasoning efforts",
                        entry.id
                    );
                }
            }
            if entry
                .default_reasoning_effort
                .as_ref()
                .is_some_and(|v| !efforts.contains(v))
            {
                bail!(
                    "model {} default reasoning effort is absent from its list",
                    entry.id
                );
            }
        }
        if entry.reasoning_discovery.as_ref().is_some_and(|record| {
            record.complete && record.source == DiscoverySource::VerifiedProbes
        }) {
            bail!("model {} probe results cannot claim completeness", entry.id);
        }
    }
    Ok(())
}

pub fn validate_catalog(
    models: &[CatalogModel],
    providers: &HashMap<String, ModelProviderInfo>,
) -> Result<()> {
    validate_models(models)?;
    crate::config_toml::validate_model_providers(providers).map_err(anyhow::Error::msg)?;
    for model in models {
        if !providers.contains_key(&model.provider) {
            bail!(
                "model {} references missing provider {}",
                model.id,
                model.provider
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CatalogSnapshot {
    pub models: Vec<CatalogModel>,
    pub providers: HashMap<String, ModelProviderInfo>,
    pub version: String,
    pub source: PathBuf,
}

#[derive(Deserialize)]
struct CatalogFileConfig {
    #[serde(default)]
    models: Vec<CatalogModel>,
    #[serde(default)]
    model_providers: HashMap<String, ModelProviderInfo>,
}

impl CatalogSnapshot {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = read_config(path)?;
        Self::from_raw(path, &raw)
    }

    fn from_raw(path: &Path, raw: &str) -> Result<Self> {
        let parsed = effective_config(path, raw)?;
        let providers = xdcode_model_provider_info::merge_configured_model_providers(
            HashMap::new(),
            parsed.model_providers,
        )
        .map_err(anyhow::Error::msg)?;
        let mut models = parsed.models;
        for model in &mut models {
            if let Some(provider) = providers.get(&model.provider) {
                model.reconcile_discovery(provider);
            }
        }
        validate_models(&models)?;
        Ok(Self {
            models,
            providers,
            version: content_version(raw),
            source: path.to_owned(),
        })
    }

    /// Re-read enabled file layers when opening /model, retaining session overrides and
    /// the runtime loader's established trust/disable decisions.
    pub fn load_layered(stack: &crate::ConfigLayerStack) -> Result<Self> {
        let target = stack
            .get_user_config_file()
            .context("no writable user configuration")?;
        let mut merged = toml::Value::Table(toml::map::Map::new());
        for layer in stack.layers_low_to_high() {
            let path = layer_file(&layer.name);
            let value = match path {
                Some(path) => toml::from_str::<toml::Value>(&read_config(&path)?)?,
                None => layer.config.clone(),
            };
            crate::merge_toml_values(&mut merged, &value);
        }
        let requirements =
            crate::model_provider_requirements::to_config(stack.requirements_toml())?;
        crate::model_provider_requirements::apply(&mut merged, &requirements);
        let parsed: crate::config_toml::ConfigToml = merged.try_into()?;
        let source = catalog_write_target(stack).unwrap_or_else(|_| target.as_path().to_owned());
        let mut snapshot = Self::load(&source)?;
        snapshot.models = parsed.models;
        snapshot.providers = xdcode_model_provider_info::merge_configured_model_providers(
            HashMap::new(),
            parsed.model_providers,
        )
        .map_err(anyhow::Error::msg)?;
        for model in &mut snapshot.models {
            if let Some(provider) = snapshot.providers.get(&model.provider) {
                model.reconcile_discovery(provider);
            }
        }
        Ok(snapshot)
    }

    pub fn model(&self, id: &str) -> Result<&CatalogModel> {
        self.models
            .iter()
            .find(|model| model.id == id)
            .with_context(|| format!("unknown model directory ID: {id}"))
    }

    /// Deliberately expose only routing names and credential presence, never provider secrets.
    pub fn redacted_json(&self) -> serde_json::Value {
        let providers: serde_json::Map<String, serde_json::Value> = self.providers.iter().map(|(id, provider)| {
            (id.clone(), serde_json::json!({
                "name": provider.name,
                "wire_api": provider.wire_api,
                "base_url_configured": provider.base_url.is_some(),
                "endpoint": provider.base_url.as_ref().map(|url| redacted_endpoint(url)),
                "env_key": provider.env_key,
                "credentials_present": provider.api_key().is_ok(),
                "has_inline_credentials": provider.experimental_bearer_token.is_some(),
                "has_command_auth": provider.auth.is_some(),
                "headers_configured": provider.http_headers.is_some() || provider.env_http_headers.is_some()
            }))
        }).collect();
        serde_json::json!({"models": self.models, "providers": providers, "version": self.version, "source": self.source})
    }
}

/// Origins and known public API path prefixes are useful for recognizing/reusing a
/// provider. Arbitrary paths may embed credentials, so disclose only their fingerprint.
fn redacted_endpoint(raw: &str) -> serde_json::Value {
    match reqwest::Url::parse(raw) {
        Ok(url) => {
            let path = url.path();
            let public_path = matches!(
                path,
                "" | "/" | "/v1" | "/v1/" | "/api/v1" | "/api/v1/" | "/openai/v1" | "/openai/v1/"
            );
            serde_json::json!({
                "origin": url.origin().ascii_serialization(),
                "path": if public_path { path } else { "<redacted>" },
                "fingerprint": content_version(raw),
            })
        }
        Err(_) => serde_json::json!({"invalid": true, "fingerprint": content_version(raw)}),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogUpsert {
    #[serde(default)]
    pub models: Vec<CatalogModel>,
    #[serde(default)]
    pub providers: HashMap<String, ModelProviderInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogWriteResult {
    pub version: String,
    pub changed: bool,
    pub preview: bool,
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Ready,
    Unverified,
    MissingCredentials,
    Incompatible,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCheck {
    pub id: String,
    pub status: ModelStatus,
    pub issues: Vec<String>,
    pub missing_credentials: Vec<String>,
}

pub fn static_check(snapshot: &CatalogSnapshot, id: &str) -> Result<ModelCheck> {
    let model = snapshot.model(id)?;
    let mut check = ModelCheck {
        id: id.into(),
        status: ModelStatus::Unverified,
        issues: Vec::new(),
        missing_credentials: Vec::new(),
    };
    if model.compatibility == ModelCompatibility::Incompatible {
        check.status = ModelStatus::Incompatible;
        check
            .issues
            .push("model is marked incompatible with the Responses API".into());
    }
    let Some(provider) = snapshot.providers.get(&model.provider) else {
        check.status = ModelStatus::Incompatible;
        check
            .issues
            .push(format!("missing provider {}", model.provider));
        return Ok(check);
    };
    if provider
        .base_url
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        check.status = ModelStatus::Incompatible;
        check
            .issues
            .push("provider requires an explicit Responses base_url".into());
    }
    if let Some(key) = &provider.env_key
        && std::env::var(key).ok().is_none_or(|v| v.trim().is_empty())
    {
        check.missing_credentials.push(key.clone());
        if check.status != ModelStatus::Incompatible {
            check.status = ModelStatus::MissingCredentials;
        }
    }
    for env in provider
        .env_http_headers
        .iter()
        .flat_map(|headers| headers.values())
    {
        if std::env::var(env).ok().is_none_or(|v| v.trim().is_empty()) {
            check.missing_credentials.push(env.clone());
            if check.status != ModelStatus::Incompatible {
                check.status = ModelStatus::MissingCredentials;
            }
        }
    }
    if check.status == ModelStatus::Unverified
        && model.compatibility == ModelCompatibility::Responses
    {
        check.status = ModelStatus::Ready;
    }
    Ok(check)
}

/// JSON command input must use credential references rather than embedding secrets.
pub fn validate_upsert_credentials(update: &CatalogUpsert) -> Result<()> {
    for provider in update.providers.values() {
        if provider.experimental_bearer_token.is_some()
            || provider.http_headers.is_some()
            || provider.query_params.is_some()
        {
            bail!(
                "model upsert accepts env_key/env_http_headers credential references; inline credentials, headers and query parameters must be configured outside JSON"
            );
        }
        if let Some(url) = &provider.base_url {
            let parsed = reqwest::Url::parse(url).context("invalid provider base_url")?;
            if !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                bail!(
                    "provider base_url must not contain credentials, query parameters or fragments"
                );
            }
        }
        if provider.gateway_oauth.is_some() || provider.aws.is_some() {
            bail!(
                "configure OAuth/AWS credentials through the existing credential mechanism rather than model JSON"
            );
        }
        if let Some(auth) = &provider.auth {
            // Command arguments can contain credentials. Commands are configured through the
            // existing credential mechanism, never through model-directory JSON commands.
            let _ = auth;
            bail!(
                "configure command-backed authentication through the existing credential mechanism"
            );
        }
    }
    Ok(())
}

/// Merge by stable ID, preserving all unrelated TOML and comments. The supplied version is
/// an optimistic concurrency guard; discovery uses the same guard after re-reading/merging.
pub fn upsert_catalog(
    path: &Path,
    update: &CatalogUpsert,
    expected_version: Option<&str>,
    preview: bool,
) -> Result<CatalogWriteResult> {
    upsert_catalog_with_context(path, update, expected_version, preview, None)
}

/// Mutate the effective directory's source, so an added entry is immediately visible.
/// Provider definitions inherited from other enabled layers participate in validation.
pub fn upsert_catalog_layered(
    stack: &crate::ConfigLayerStack,
    update: &CatalogUpsert,
    expected_version: Option<&str>,
    preview: bool,
) -> Result<CatalogWriteResult> {
    let path = catalog_write_target(stack)?;
    let snapshot = CatalogSnapshot::load_layered(stack)?;
    for id in update.providers.keys() {
        if stack
            .requirements_toml()
            .model_providers
            .as_ref()
            .is_some_and(|providers| providers.contains_key(id))
        {
            bail!(
                "provider {id} is controlled by managed requirements and cannot be changed by model upsert"
            );
        }
    }
    // An existing provider overridden above the model directory cannot be changed by
    // writing below that override. Refuse that ambiguous operation before any mutation.
    let mut above_target = false;
    for layer in stack.layers_low_to_high() {
        let file = layer_file(&layer.name);
        if file.as_deref() == Some(path.as_path()) {
            above_target = true;
            continue;
        }
        if above_target {
            let value = match file {
                Some(file) => toml::from_str::<toml::Value>(&read_config(&file)?)?,
                None => layer.config.clone(),
            };
            for id in update.providers.keys() {
                if value
                    .get("model_providers")
                    .and_then(|providers| providers.get(id))
                    .is_some()
                {
                    bail!(
                        "provider {id} is overridden above the directory source; edit that provider's defining configuration first"
                    );
                }
            }
        }
    }
    upsert_catalog_with_context(
        &path,
        update,
        expected_version,
        preview,
        Some(&snapshot.providers),
    )
}

fn upsert_catalog_with_context(
    path: &Path,
    update: &CatalogUpsert,
    expected_version: Option<&str>,
    preview: bool,
    context: Option<&HashMap<String, ModelProviderInfo>>,
) -> Result<CatalogWriteResult> {
    validate_upsert_credentials(update)?;
    edit_catalog(path, expected_version, preview, |doc| {
        if doc.get("models").is_none() {
            let inherited = effective_config(path, &doc.to_string())?.models;
            for model in &inherited {
                set_model(doc, model)?;
            }
        }
        for model in &update.models {
            set_model(doc, model)?;
        }
        if !update.providers.is_empty() {
            if doc.get("model_providers").is_none() {
                doc["model_providers"] = Item::Table(Table::new());
            }
            for (id, provider) in &update.providers {
                let mut serialized = toml::to_string(provider)?.parse::<DocumentMut>()?;
                let target = &mut doc["model_providers"][id];
                // ModelProviderInfo represents omitted booleans as false. For an existing
                // provider JSON upserts are patches, so omitted default flags cannot
                // silently disable websocket or other established capabilities.
                if !target.is_none() {
                    for key in [
                        "requires_hosted_auth",
                        "supports_websockets",
                        "supports_standalone_web_search",
                    ] {
                        if serialized.get(key).and_then(Item::as_bool) == Some(false) {
                            serialized.remove(key);
                        }
                    }
                }
                if target.is_none() {
                    *target = Item::Table(Table::new());
                }
                merge_table(
                    target
                        .as_table_mut()
                        .context("provider must be a TOML table")?,
                    serialized.as_table(),
                );
            }
        }
        let result = effective_config(path, &doc.to_string())?;
        let mut providers = match context {
            Some(context) => context.clone(),
            None => result.model_providers.clone(),
        };
        if context.is_some() {
            for id in update.providers.keys() {
                if let Some(provider) = result.model_providers.get(id) {
                    providers.insert(id.clone(), provider.clone());
                }
            }
        }
        validate_catalog(&result.models, &providers)
    })
}

fn merge_table(target: &mut Table, source: &Table) {
    for (key, item) in source.iter() {
        if let Some(existing) = target.get_mut(key).and_then(Item::as_table_mut) {
            if let Some(incoming) = item.as_table() {
                merge_table(existing, incoming);
                continue;
            }
        }
        let mut replacement = item.clone();
        // Parsed positions belong to the temporary serialized document. Reusing
        // them here can emit a model's discovery table before its [[models]]
        // parent, attaching it to another entry or producing duplicate keys.
        clear_table_positions(&mut replacement);
        if let Some(old) = target.get(key)
            && let (Some(old), Some(new)) = (old.as_value(), replacement.as_value_mut())
        {
            *new.decor_mut() = old.decor().clone();
        }
        target.insert(key, replacement);
    }
}

fn clear_table_positions(item: &mut Item) {
    if let Some(table) = item.as_table_mut() {
        table.set_position(None);
        for (_, child) in table.iter_mut() {
            clear_table_positions(child);
        }
    } else if let Some(tables) = item.as_array_of_tables_mut() {
        for table in tables.iter_mut() {
            table.set_position(None);
            for (_, child) in table.iter_mut() {
                clear_table_positions(child);
            }
        }
    }
}

fn set_model(doc: &mut DocumentMut, model: &CatalogModel) -> Result<()> {
    if doc.get("models").is_none() {
        doc["models"] = Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let tables = doc["models"]
        .as_array_of_tables_mut()
        .context("models must be [[models]] tables")?;
    let mut found = None;
    for (idx, table) in tables.iter().enumerate() {
        if table.get("id").and_then(Item::as_str) == Some(model.id.as_str()) {
            found = Some(idx);
            break;
        }
    }
    if found.is_none() {
        tables.push(Table::new());
    }
    let idx = found.unwrap_or(tables.len() - 1);
    let target = tables.get_mut(idx).context("missing model table")?;
    let serialized = toml::to_string(model)?.parse::<DocumentMut>()?;
    for key in [
        "reasoning_efforts",
        "default_reasoning_effort",
        "reasoning_discovery",
        "context_window",
        "auto_compact_token_limit",
    ] {
        if !serialized.contains_key(key) {
            target.remove(key);
        }
    }
    merge_table(target, serialized.as_table());
    Ok(())
}

fn edit_catalog(
    path: &Path,
    expected: Option<&str>,
    preview: bool,
    edit: impl FnOnce(&mut DocumentMut) -> Result<()>,
) -> Result<CatalogWriteResult> {
    let mut transaction = ConfigurationFile::open(path)?;
    transaction.check_version(expected, false)?;
    edit(transaction.document())?;
    transaction.finish(preview)
}

pub struct ConfigurationFile {
    path: PathBuf,
    read_path: Option<PathBuf>,
    write_path: PathBuf,
    original: String,
    document: DocumentMut,
    _lock: CatalogLock,
}

impl ConfigurationFile {
    pub fn open(path: &Path) -> Result<Self> {
        let paths = resolve_symlink_write_paths(path)?;
        if let Some(parent) = paths.write_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = CatalogLock::acquire(paths.write_path.with_extension("toml.models.lock"))?;
        let original = match &paths.read_path {
            Some(read_path) => read_config(read_path)?,
            None => String::new(),
        };
        let document = original.parse::<DocumentMut>()?;
        Ok(Self {
            path: path.to_path_buf(),
            read_path: paths.read_path,
            write_path: paths.write_path,
            original,
            document,
            _lock: lock,
        })
    }

    pub fn check_version(&self, expected: Option<&str>, accept_legacy: bool) -> Result<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        if expected == content_version(&self.original) {
            return Ok(());
        }
        if accept_legacy {
            let value: toml::Value = self.original.parse()?;
            if expected == crate::version_for_toml(&value) {
                return Ok(());
            }
        }
        bail!("configuration version changed; reload and retry")
    }

    pub fn document(&mut self) -> &mut DocumentMut {
        &mut self.document
    }

    pub fn finish(self, preview: bool) -> Result<CatalogWriteResult> {
        let updated = self.document.to_string();
        let changed = self.original != updated;
        let models = CatalogSnapshot::from_raw(&self.path, &updated)?.models;
        if changed && !preview {
            let paths = resolve_symlink_write_paths(&self.path)?;
            if paths.write_path != self.write_path || paths.read_path != self.read_path {
                bail!("configuration path changed while editing; reload and retry");
            }
            if fs::metadata(&self.write_path)
                .ok()
                .is_some_and(|metadata| metadata.permissions().readonly())
            {
                bail!("configuration is read-only; changes were not saved");
            }
            let current = match &paths.read_path {
                Some(read_path) => read_config(read_path)?,
                None => String::new(),
            };
            if content_version(&current) != content_version(&self.original) {
                bail!("configuration changed while editing; reload and retry");
            }
            write_atomically(&self.write_path, &updated)?;
        }
        Ok(CatalogWriteResult {
            version: content_version(&updated),
            changed,
            preview,
            models,
        })
    }
}

struct CatalogLock {
    path: PathBuf,
}
impl CatalogLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context(
                "model configuration is being edited; retry after the other writer finishes",
            )?;
        Ok(Self { path })
    }
}
impl Drop for CatalogLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn layer_file(source: &crate::ConfigLayerSource) -> Option<PathBuf> {
    use crate::ConfigLayerSource;
    match source {
        ConfigLayerSource::System { file }
        | ConfigLayerSource::User { file, .. }
        | ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => {
            Some(file.as_path().to_owned())
        }
        ConfigLayerSource::Project { dot_xdcode_folder } => {
            Some(dot_xdcode_folder.as_path().join(crate::CONFIG_TOML_FILE))
        }
        _ => None,
    }
}

/// A refresh writes the file that owns the model array. Session/managed arrays are
/// deliberately not writable by model configuration commands.
pub fn catalog_source(stack: &crate::ConfigLayerStack) -> Result<PathBuf> {
    for layer in stack.layers_high_to_low() {
        let path = layer_file(&layer.name);
        let value = match &path {
            Some(path) => toml::from_str::<toml::Value>(&read_config(path)?)?,
            None => layer.config.clone(),
        };
        if value.get("models").is_some() {
            return match &layer.name {
                crate::ConfigLayerSource::User { .. }
                | crate::ConfigLayerSource::Project { .. } => {
                    path.context("model array has no writable source")
                }
                _ => anyhow::bail!(
                    "model array belongs to a managed or session layer; refresh its writable source explicitly"
                ),
            };
        }
    }
    bail!("no model directory is configured")
}

/// The model array's defining writable file, or the active user/profile file when
/// no model array exists. A managed/session override cannot be silently shadowed.
pub fn catalog_write_target(stack: &crate::ConfigLayerStack) -> Result<PathBuf> {
    for layer in stack.layers_high_to_low() {
        let path = layer_file(&layer.name);
        let value = match &path {
            Some(path) => toml::from_str::<toml::Value>(&read_config(path)?)?,
            None => layer.config.clone(),
        };
        if value.get("models").is_some() {
            return match &layer.name {
                crate::ConfigLayerSource::User { .. }
                | crate::ConfigLayerSource::Project { .. } => {
                    path.context("model array has no writable source")
                }
                _ => anyhow::bail!(
                    "model array belongs to a managed or session layer; model upsert cannot override it"
                ),
            };
        }
    }
    stack
        .get_user_config_file()
        .map(|path| path.as_path().to_owned())
        .context("no writable user configuration")
}

fn effective_config(path: &Path, raw: &str) -> Result<CatalogFileConfig> {
    let mut value: toml::Value = toml::from_str(raw).context("invalid model configuration")?;
    // Named profiles are sibling TOML files. Use the same table merge/array replacement
    // semantics as the runtime loader without moving the write target to the base file.
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".config.toml"))
    {
        if let Some(parent) = path.parent() {
            let base_path = parent.join(crate::CONFIG_TOML_FILE);
            if base_path.exists() {
                let mut base = toml::from_str::<toml::Value>(&read_config(&base_path)?)?;
                crate::merge_toml_values(&mut base, &value);
                value = base;
            }
        }
    }
    value.try_into().context("invalid model configuration")
}

fn read_config(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn content_version(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}
fn efforts_fingerprint(efforts: &[ReasoningEffort]) -> String {
    content_version(&serde_json::to_string(efforts).unwrap_or_default())
}
/// The hash contains secrets only as one-way input and can safely be persisted.
pub fn interface_fingerprint(provider: &ModelProviderInfo) -> String {
    // Sort all maps for a stable hash across reloads.
    let mut json = serde_json::to_value(provider).unwrap_or_default();
    json.sort_all_objects();
    content_version(&json.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryRefresh {
    pub model: CatalogModel,
    pub saved: bool,
    pub message: Option<String>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
