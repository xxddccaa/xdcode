//! Browse and edit user-owned providers through the owning App Server's configuration APIs.

use super::App;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::backend_id::BackendId;
use crate::bottom_pane::ListSelectionView;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::tui::Tui;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::watch;
use uuid::Uuid;
use xdcode_app_server_protocol::ClientRequest;
use xdcode_app_server_protocol::ConfigBatchWriteParams;
use xdcode_app_server_protocol::ConfigEdit;
use xdcode_app_server_protocol::ConfigLayerSource;
use xdcode_app_server_protocol::ConfigReadParams;
use xdcode_app_server_protocol::ConfigReadResponse;
use xdcode_app_server_protocol::ConfigWriteResponse;
use xdcode_app_server_protocol::MergeStrategy;
use xdcode_app_server_protocol::RequestId;

mod editor;

const LOADING_VIEW: &str = "model_provider_loading";
const LIST_VIEW: &str = "model_provider_list";

#[derive(Clone, Debug)]
pub(crate) struct Provider {
    id: String,
    name: String,
    values: BTreeMap<Field, String>,
    editable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    providers: Vec<Provider>,
    file: Option<String>,
    version: Option<String>,
}

impl Catalog {
    fn from_response(response: ConfigReadResponse) -> Self {
        let user = response.layers.as_ref().and_then(|layers| {
            layers.iter().find(|layer| {
                matches!(layer.name, ConfigLayerSource::User { .. })
                    && layer.disabled_reason.is_none()
            })
        });
        let file = user.and_then(|layer| match &layer.name {
            ConfigLayerSource::User { file, .. } => Some(file.to_string_lossy().into_owned()),
            _ => None,
        });
        let version = user.map(|layer| layer.version.clone());
        let providers = response
            .config
            .additional
            .get("model_providers")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .map(|(id, definition)| {
                let values = [Field::Name, Field::Url, Field::Variable]
                    .into_iter()
                    .filter_map(|field| {
                        definition
                            .get(field.key())
                            .and_then(serde_json::Value::as_str)
                            .map(|value| (field, value.to_string()))
                    })
                    .collect::<BTreeMap<_, _>>();
                let owned = user
                    .and_then(|layer| layer.config.get("model_providers"))
                    .and_then(|providers| providers.get(id))
                    .is_some();
                let managed = response.origins.iter().any(|(path, origin)| {
                    (path == &format!("model_providers.{id}")
                        || path.starts_with(&format!("model_providers.{id}.")))
                        && !matches!(origin.name, ConfigLayerSource::User { .. })
                });
                let responses = definition
                    .get("wire_api")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|wire| wire == "responses");
                Provider {
                    id: id.clone(),
                    name: values
                        .get(&Field::Name)
                        .cloned()
                        .unwrap_or_else(|| id.clone()),
                    values,
                    editable: owned && !managed && responses,
                }
            })
            .collect();
        Self {
            providers,
            file,
            version,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Field {
    Id,
    Name,
    Url,
    Variable,
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::Url => "base_url",
            Self::Variable => "env_key",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Id => "Provider ID",
            Self::Name => "Name",
            Self::Url => "Base URL",
            Self::Variable => "API key environment variable",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Draft {
    catalog: Catalog,
    provider: Option<Provider>,
    changes: BTreeMap<Field, String>,
}

impl Draft {
    fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Name, Field::Url, Field::Variable];
        if self.provider.is_none() {
            fields.insert(0, Field::Id);
        }
        fields
    }
    fn value(&self, field: Field) -> &str {
        self.changes
            .get(&field)
            .or_else(|| {
                self.provider
                    .as_ref()
                    .and_then(|provider| provider.values.get(&field))
            })
            .map(String::as_str)
            .unwrap_or_default()
    }
    fn patch(&self) -> Result<ConfigBatchWriteParams, String> {
        let file = self
            .catalog
            .file
            .clone()
            .ok_or("No writable user configuration layer is available.")?;
        if self
            .provider
            .as_ref()
            .is_some_and(|provider| !provider.editable)
        {
            return Err(
                "This provider is read-only. Edit its owning configuration layer instead.".into(),
            );
        }
        let id = self
            .provider
            .as_ref()
            .map(|provider| provider.id.as_str())
            .unwrap_or_else(|| self.value(Field::Id));
        if id.is_empty()
            || (self.provider.is_none()
                && !id.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                }))
        {
            return Err(
                "Use a nonempty provider ID containing letters, numbers, '-' or '_'.".into(),
            );
        }
        if self.provider.is_none()
            && self
                .catalog
                .providers
                .iter()
                .any(|provider| provider.id == id)
        {
            return Err(
                "That provider ID already exists. Edit the existing provider instead.".into(),
            );
        }
        if self.value(Field::Name).trim().is_empty() {
            return Err("Enter a provider name.".into());
        }
        if self.provider.is_none() || self.changes.contains_key(&Field::Url) {
            let url = url::Url::parse(self.value(Field::Url))
                .map_err(|_| "Enter a valid HTTP(S) base URL.")?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("Use an HTTP(S) base URL without embedded credentials.".into());
            }
        }
        let variable = self.value(Field::Variable);
        if !variable.is_empty()
            && (variable
                .chars()
                .next()
                .is_none_or(|character| character.is_ascii_digit())
                || !variable
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_'))
        {
            return Err("Enter an environment variable name, not a credential value.".into());
        }
        let mut edits = self
            .changes
            .iter()
            .filter(|(field, _)| **field != Field::Id)
            .map(|(field, value)| ConfigEdit {
                key_path: format!("model_providers.{id:?}.{}", field.key()),
                value: if *field == Field::Variable && value.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(value)
                },
                merge_strategy: MergeStrategy::Replace,
            })
            .collect::<Vec<_>>();
        if self.provider.is_none() {
            edits.extend(
                [
                    ("wire_api", serde_json::json!("responses")),
                    ("requires_hosted_auth", serde_json::json!(false)),
                ]
                .map(|(field, value)| ConfigEdit {
                    key_path: format!("model_providers.{id:?}.{field}"),
                    value,
                    merge_strategy: MergeStrategy::Replace,
                }),
            );
        }
        if edits.is_empty() {
            return Err("No provider fields have changed.".into());
        }
        Ok(ConfigBatchWriteParams {
            edits,
            file_path: Some(file),
            expected_version: self.catalog.version.clone(),
            reload_user_config: false,
        })
    }
    fn rebase(&mut self, catalog: Catalog) {
        if let Some(provider) = &self.provider {
            self.provider = catalog
                .providers
                .iter()
                .find(|current| current.id == provider.id)
                .cloned()
                .or_else(|| {
                    Some(Provider {
                        editable: false,
                        ..provider.clone()
                    })
                });
        }
        self.catalog = catalog;
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    Load(watch::Receiver<()>),
    Loaded {
        backend: BackendId,
        dismissed: watch::Receiver<()>,
        result: Result<Catalog, String>,
    },
    Edit {
        backend: BackendId,
        draft: Draft,
    },
    Save {
        backend: BackendId,
        draft: Draft,
        alive: Arc<AtomicBool>,
    },
    Saved {
        backend: BackendId,
        draft: Draft,
        alive: Arc<AtomicBool>,
        result: Result<ConfigWriteResponse, String>,
        refreshed: Result<Catalog, String>,
    },
}

fn back() -> SelectionItem {
    SelectionItem {
        name: "Back".into(),
        dismiss_on_select: true,
        ..Default::default()
    }
}

pub(crate) fn loading() -> (SelectionViewParams, Event) {
    let (dismiss, dismissed) = watch::channel(());
    (
        SelectionViewParams {
            view_id: Some(LOADING_VIEW),
            title: Some("Loading model providers…".into()),
            items: vec![back()],
            allow_input_when_disconnected: true,
            on_cancel: Some(Box::new(move |_| {
                dismiss.send_replace(());
            })),
            ..Default::default()
        },
        Event::Load(dismissed),
    )
}

fn list(backend: BackendId, catalog: Catalog, notice: Option<String>) -> SelectionViewParams {
    let mut items = catalog
        .providers
        .iter()
        .map(|provider| {
            let draft = Draft {
                catalog: catalog.clone(),
                provider: Some(provider.clone()),
                changes: BTreeMap::new(),
            };
            SelectionItem {
                name: format!("{} ({})", provider.name, provider.id),
                description: Some(if provider.editable {
                    provider
                        .values
                        .get(&Field::Url)
                        .cloned()
                        .unwrap_or_else(|| "Configured provider".into())
                } else {
                    "Read-only provider; configuration and credentials remain server-owned".into()
                }),
                dismiss_on_select: false,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::Provider(Event::Edit {
                        backend,
                        draft: draft.clone(),
                    }))
                })],
                ..Default::default()
            }
        })
        .collect::<Vec<_>>();
    if catalog.file.is_some() {
        items.push(SelectionItem {
            name: "Add Responses provider".into(),
            dismiss_on_select: false,
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::Provider(Event::Edit {
                    backend,
                    draft: Draft {
                        catalog: catalog.clone(),
                        provider: None,
                        changes: BTreeMap::new(),
                    },
                }))
            })],
            ..Default::default()
        });
    }
    items.push(back());
    SelectionViewParams {
        view_id: Some(LIST_VIEW),
        title: Some("Model providers".into()),
        subtitle: notice.or_else(|| {
            Some("Edits do not switch the active chat or project model preferences.".into())
        }),
        items,
        allow_input_when_disconnected: true,
        ..Default::default()
    }
}

async fn read(
    handle: &xdcode_app_server_client::AppServerRequestHandle,
) -> Result<Catalog, String> {
    let response: ConfigReadResponse = handle
        .request_typed(ClientRequest::ConfigRead {
            request_id: RequestId::String(Uuid::new_v4().to_string()),
            params: ConfigReadParams {
                include_layers: true,
                cwd: None,
            },
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(Catalog::from_response(response))
}

pub(super) fn handle(app: &mut App, _tui: &mut Tui, server: &mut AppServerSession, event: Event) {
    let backend = server.backend_id();
    match event {
        Event::Load(mut dismissed) => {
            let tx = app.app_event_tx.clone();
            let handle = server.request_handle();
            let offline = app.reconnect.offline;
            tokio::spawn(async move {
                let result = if offline {
                    Err("Reconnect to the app server to browse providers.".into())
                } else {
                    tokio::select! { _ = dismissed.changed() => return, result = read(&handle) => result }
                };
                tx.send(AppEvent::Provider(Event::Loaded {
                    backend,
                    dismissed,
                    result,
                }));
            });
        }
        Event::Loaded {
            backend: origin,
            dismissed,
            result,
        } => {
            if origin != backend || !matches!(dismissed.has_changed(), Ok(false)) {
                return;
            }
            let view = match result {
                Ok(catalog) => list(backend, catalog, None),
                Err(error) => SelectionViewParams {
                    title: Some("Model providers".into()),
                    subtitle: Some(error),
                    items: vec![back()],
                    allow_input_when_disconnected: true,
                    ..Default::default()
                },
            };
            app.chat_widget
                .replace_selection_view_if_present(LOADING_VIEW, view);
        }
        Event::Edit {
            backend: origin,
            draft,
        } => {
            if origin != backend {
                return;
            }
            app.chat_widget
                .show_bottom_pane_view(editor::view(app, backend, draft, None));
        }
        Event::Save {
            backend: origin,
            draft,
            alive,
        } => {
            if origin != backend || !alive.load(Ordering::Acquire) {
                return;
            }
            let params = match draft.patch() {
                Ok(params) => params,
                Err(error) => {
                    let view = editor::view(app, backend, draft, Some(error));
                    app.chat_widget
                        .replace_bottom_pane_view_if_present(editor::VIEW_ID, view);
                    return;
                }
            };
            let handle = server.request_handle();
            let tx = app.app_event_tx.clone();
            let offline = app.reconnect.offline;
            tokio::spawn(async move {
                let result = if offline {
                    Err("Disconnected: provider changes were not sent.".into())
                } else {
                    handle
                        .request_typed(ClientRequest::ConfigBatchWrite {
                            request_id: RequestId::String(Uuid::new_v4().to_string()),
                            params,
                        })
                        .await
                        .map_err(|error| error.to_string())
                };
                let refreshed = if offline {
                    Err("Reconnect before retrying.".into())
                } else {
                    read(&handle).await
                };
                tx.send(AppEvent::Provider(Event::Saved {
                    backend,
                    draft,
                    alive,
                    result,
                    refreshed,
                }));
            });
        }
        Event::Saved {
            backend: origin,
            mut draft,
            alive,
            result,
            refreshed,
        } => {
            if origin != backend || !alive.load(Ordering::Acquire) {
                return;
            }
            match result {
                Ok(_) => {
                    let (catalog, notice) = match refreshed {
                        Ok(catalog) => (
                            catalog,
                            "Saved. The current chat and Default/Plan selections are unchanged."
                                .into(),
                        ),
                        Err(error) => (
                            draft.catalog,
                            format!("Saved, but catalog refresh failed: {error}"),
                        ),
                    };
                    let view = ListSelectionView::new(
                        list(backend, catalog, Some(notice)),
                        app.app_event_tx.clone(),
                        app.keymap.list.clone(),
                    );
                    app.chat_widget
                        .replace_bottom_pane_view_if_present(editor::VIEW_ID, Box::new(view));
                    server.fetch_models(Uuid::new_v4(), app.app_event_tx.clone());
                }
                Err(error) => {
                    if let Ok(catalog) = refreshed {
                        draft.rebase(catalog);
                    }
                    let view = editor::view(
                        app,
                        backend,
                        draft,
                        Some(format!(
                            "Save was not confirmed: {error}. Draft retained; review and save explicitly to retry."
                        )),
                    );
                    app.chat_widget
                        .replace_bottom_pane_view_if_present(editor::VIEW_ID, view);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
