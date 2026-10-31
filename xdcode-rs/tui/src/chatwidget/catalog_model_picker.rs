//! The user catalog picker. Every final choice goes through one acknowledged server update.
use super::*;
use xdcode_config::model_catalog::CatalogModel;
use xdcode_config::model_catalog::CatalogSnapshot;
use xdcode_config::model_catalog::ModelStatus;
use xdcode_config::model_catalog::static_check;
use xdcode_protocol::protocol::CatalogModelSelection;

const CATALOG_MODEL_VIEW: &str = "catalog-model-selection";

fn provider_sort_key(snapshot: &CatalogSnapshot, provider: &str) -> (String, String) {
    let label = snapshot
        .providers
        .get(provider)
        .map_or(provider, |entry| entry.name.as_str());
    (label.to_lowercase(), provider.to_owned())
}

#[derive(Default)]
pub(super) struct CatalogSelectionState {
    current_id: Option<String>,
    normal: Option<CatalogModelSelection>,
    plan: Option<CatalogModelSelection>,
    legacy_normal: Option<CollaborationMode>,
    legacy_plan: Option<CollaborationMode>,
    discovered: std::collections::HashMap<String, CatalogModel>,
}

impl ChatWidget {
    pub(crate) fn apply_confirmed_catalog_selection(&mut self, selection: &CatalogModelSelection) {
        self.catalog_selection_state.current_id = Some(selection.catalog_id.clone());
        match selection.mode {
            ModeKind::Default => self.catalog_selection_state.normal = Some(selection.clone()),
            ModeKind::Plan => self.catalog_selection_state.plan = Some(selection.clone()),
        }
        self.config.model_provider_id = selection.provider.clone();
        if let Ok(snapshot) = self.user_model_catalog()
            && let Some(provider) = snapshot.providers.get(&selection.provider)
        {
            self.config.model_provider = provider.clone();
        }
        let mut mode =
            collaboration_modes::mask_for_kind(self.model_catalog.as_ref(), selection.mode)
                .map(|mask| self.current_collaboration_mode.apply_mask(&mask))
                .unwrap_or_else(|| self.effective_collaboration_mode());
        mode.mode = selection.mode;
        mode.settings.developer_instructions = None;
        mode.settings.model = selection.model.clone();
        mode.settings.reasoning_effort = selection.effort.clone();
        self.set_effective_collaboration_mode(mode);
    }

    pub(crate) fn current_catalog_id(&self) -> Option<&str> {
        self.catalog_selection_state.current_id.as_deref()
    }

    fn current_catalog_entry<'a>(
        &self,
        snapshot: &'a CatalogSnapshot,
    ) -> Result<Option<&'a CatalogModel>, String> {
        let matches: Vec<_> = snapshot
            .models
            .iter()
            .filter(|entry| {
                entry.provider == self.config.model_provider_id
                    && entry.model == self.current_model()
            })
            .collect();
        if let Some(id) = self.current_catalog_id() {
            return matches.iter().find(|entry| entry.id == id).copied().map(Some)
                .ok_or_else(|| "The active catalog entry was removed or changed. The current selection is unchanged; choose an entry with /model before switching modes.".into());
        }
        match matches.as_slice() {
            [] => Ok(None),
            [entry] => Ok(Some(entry)),
            _ => Err("The current request model matches multiple catalog IDs. Choose the intended entry with /model before switching modes.".into()),
        }
    }

    pub(crate) fn restore_catalog_mode_memory(
        &mut self,
        memory: &crate::project_model_memory::ProjectModelMemory,
    ) {
        self.catalog_selection_state.normal = memory.normal.clone();
        self.catalog_selection_state.plan = memory.plan.clone();
    }

    pub(super) fn reset_catalog_session_state(&mut self) {
        self.catalog_selection_state = Default::default();
    }

    pub(super) fn sync_catalog_identity(&mut self, id: Option<String>) {
        self.catalog_selection_state.current_id = id;
        self.sync_catalog_provider();
    }

    fn sync_catalog_provider(&mut self) {
        let provider = self
            .user_model_catalog()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .providers
                    .get(&self.config.model_provider_id)
                    .cloned()
            })
            .or_else(|| {
                self.config
                    .model_providers
                    .get(&self.config.model_provider_id)
                    .cloned()
            });
        if let Some(provider) = provider {
            self.config.model_provider = provider;
        }
    }

    /// Legacy selections still remember their actual models instead of the built-in Plan model.
    pub(crate) fn legacy_mode_mask(
        &mut self,
        mut mask: CollaborationModeMask,
    ) -> CollaborationModeMask {
        let current = self.effective_collaboration_mode();
        match current.mode {
            ModeKind::Default => self.catalog_selection_state.legacy_normal = Some(current.clone()),
            ModeKind::Plan => self.catalog_selection_state.legacy_plan = Some(current.clone()),
        }
        let kind = mask.mode.unwrap_or(ModeKind::Default);
        let remembered = match kind {
            ModeKind::Default => self.catalog_selection_state.legacy_normal.as_ref(),
            ModeKind::Plan => self.catalog_selection_state.legacy_plan.as_ref(),
        };
        let settings = remembered.unwrap_or(&current).settings.clone();
        mask.model = Some(settings.model.clone());
        let supports = |effort: &ReasoningEffortConfig| {
            self.model_catalog
                .models
                .iter()
                .find(|preset| preset.model == settings.model)
                .is_some_and(|preset| {
                    preset
                        .supported_reasoning_efforts
                        .iter()
                        .any(|option| &option.effort == effort)
                })
        };
        mask.reasoning_effort = Some(if kind == ModeKind::Plan && remembered.is_none() {
            self.config
                .plan_mode_reasoning_effort
                .clone()
                .filter(supports)
                .or(settings.reasoning_effort)
        } else {
            settings.reasoning_effort
        });
        mask
    }

    pub(super) fn try_switch_catalog_mode(&mut self, mode: ModeKind) -> bool {
        match self.catalog_mode_selection(mode) {
            Ok(Some(selection)) => {
                self.app_event_tx
                    .send(AppEvent::SwitchCatalogMode { selection });
                true
            }
            Ok(None) => false,
            Err(message) => {
                self.add_error_message(message);
                true
            }
        }
    }

    /// A fallback preserves the current provider, exact catalog ID and effort, changing only mode.
    pub(crate) fn catalog_mode_fallback_selection(
        &self,
        mode: ModeKind,
    ) -> Option<CatalogModelSelection> {
        if mode == self.effective_collaboration_mode().mode {
            return None;
        }
        let snapshot = self.user_model_catalog().ok()?;
        let entry = self.current_catalog_entry(&snapshot).ok()??;
        Some(CatalogModelSelection {
            provider: entry.provider.clone(),
            catalog_id: entry.id.clone(),
            model: entry.model.clone(),
            effort: self.current_reasoning_effort(),
            mode,
        })
    }

    pub(crate) fn catalog_mode_selection(
        &mut self,
        mode: ModeKind,
    ) -> Result<Option<CatalogModelSelection>, String> {
        let snapshot = match self.user_model_catalog() {
            Ok(snapshot) => snapshot,
            Err(error) if self.current_catalog_id().is_some() => {
                return Err(format!(
                    "Cannot refresh model configuration: {error}. The current selection is unchanged."
                ));
            }
            Err(_) => return Ok(None),
        };
        if snapshot.models.is_empty() {
            return if self.current_catalog_id().is_some() {
                Err("The active catalog entry was removed. Choose an entry with /model before changing modes.".into())
            } else {
                Ok(None)
            };
        }
        let current_entry = match self.current_catalog_entry(&snapshot) {
            Ok(entry) => entry.cloned(),
            Err(message) => {
                return Err(message);
            }
        };
        // A configured legacy model outside the directory still inherits its normal Plan behavior.
        if current_entry.is_none() {
            return Ok(None);
        }
        let current_mode = self.effective_collaboration_mode().mode;
        if let Some(entry) = &current_entry {
            let current = CatalogModelSelection {
                provider: entry.provider.clone(),
                catalog_id: entry.id.clone(),
                model: entry.model.clone(),
                effort: self.current_reasoning_effort(),
                mode: current_mode,
            };
            match current_mode {
                ModeKind::Default => self.catalog_selection_state.normal = Some(current),
                ModeKind::Plan => self.catalog_selection_state.plan = Some(current),
            }
        }
        let path = crate::project_model_memory::memory_path(
            self.config.xdcode_home.as_path(),
            &self.catalog_config_path(),
            self.config.cwd.as_path(),
        );
        let memory =
            crate::project_model_memory::ProjectModelMemory::load(&path).unwrap_or_default();
        let session_selection = match mode {
            ModeKind::Default => self.catalog_selection_state.normal.as_ref(),
            ModeKind::Plan => self.catalog_selection_state.plan.as_ref(),
        };
        let remembered = match mode {
            ModeKind::Default => memory.normal.as_ref(),
            ModeKind::Plan => memory.plan.as_ref(),
        };
        let mut selection = session_selection
            .cloned()
            .or_else(|| remembered.cloned())
            .or_else(|| {
                current_entry.as_ref().map(|entry| CatalogModelSelection {
                    provider: entry.provider.clone(),
                    catalog_id: entry.id.clone(),
                    model: entry.model.clone(),
                    effort: self.current_reasoning_effort(),
                    mode,
                })
            });
        let Some(selection) = selection.as_mut() else {
            return Err("Choose a catalog model with /model before changing modes.".into());
        };
        let first_plan = mode == ModeKind::Plan
            && self.catalog_selection_state.plan.is_none()
            && memory.plan.is_none();
        selection.mode = mode;
        if first_plan {
            if let Ok(model) = snapshot.model(&selection.catalog_id) {
                selection.effort = self
                    .config
                    .plan_mode_reasoning_effort
                    .clone()
                    .filter(|effort| model.supports_effort(effort))
                    .or_else(|| {
                        selection
                            .effort
                            .clone()
                            .filter(|effort| model.supports_effort(effort))
                    });
            }
        }
        if let Ok(model) = snapshot.model(&selection.catalog_id)
            && selection
                .effort
                .as_ref()
                .is_some_and(|effort| !model.supports_effort(effort))
        {
            selection.effort = model
                .default_reasoning_effort
                .clone()
                .filter(|effort| model.supports_effort(effort));
        }
        Ok(Some(selection.clone()))
    }

    pub(crate) fn catalog_config_path(&self) -> std::path::PathBuf {
        self.config
            .config_layer_stack
            .get_user_config_file()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| self.config.xdcode_home.join("config.toml").to_path_buf())
    }

    pub(crate) fn user_model_catalog(&self) -> anyhow::Result<CatalogSnapshot> {
        let mut snapshot = if self
            .config
            .config_layer_stack
            .get_user_config_file()
            .is_some()
        {
            CatalogSnapshot::load_layered(&self.config.config_layer_stack)?
        } else {
            CatalogSnapshot::load(&self.catalog_config_path())?
        };
        for model in &mut snapshot.models {
            if model.reasoning_efforts.is_none()
                && let Some(discovered) = self.catalog_selection_state.discovered.get(&model.id)
                && discovered.provider == model.provider
                && discovered.model == model.model
            {
                let mut discovered = discovered.clone();
                if let Some(provider) = snapshot.providers.get(&model.provider) {
                    discovered.reconcile_discovery(provider);
                }
                if discovered.reasoning_efforts.is_some() {
                    *model = discovered;
                }
            }
        }
        Ok(snapshot)
    }

    /// Re-read the catalog for every invocation. Merely browsing never changes thread routing.
    pub(super) fn try_open_catalog_model_popup(&mut self) -> bool {
        match self.user_model_catalog() {
            Ok(snapshot) if !snapshot.models.is_empty() => {
                self.open_catalog_models(snapshot, None);
                true
            }
            Ok(_) => self.retain_catalog_selection_if_removed(),
            Err(error) => {
                self.add_error_message(format!("Cannot refresh model configuration: {error}"));
                true
            }
        }
    }

    fn retain_catalog_selection_if_removed(&mut self) -> bool {
        if self.current_catalog_id().is_none() {
            return false;
        }
        self.add_error_message(
            "The model catalog was removed while this session still uses a catalog selection. The current selection is unchanged. Restore a catalog entry and choose it with /model before changing model settings.".into(),
        );
        true
    }

    /// Old popups can have queued actions when an atomic directory selection succeeds.
    /// Reject their separate model/effort/default writes before any local state changes.
    pub(crate) fn reject_legacy_model_event(&mut self, event: &AppEvent) -> bool {
        if self.current_catalog_id().is_none()
            || !matches!(
                event,
                AppEvent::UpdateModel(_)
                    | AppEvent::UpdateReasoningEffort(_)
                    | AppEvent::ApplyAdvancedReasoning { .. }
                    | AppEvent::SelectSessionModel { .. }
                    | AppEvent::PersistModelSelection { .. }
                    | AppEvent::UpdatePlanModeReasoningEffort(_)
                    | AppEvent::PersistPlanModeReasoningEffort(_)
                    | AppEvent::UpdateLunaReserveReasoning { .. }
            )
        {
            return false;
        }
        self.add_error_message(
            "This session uses a catalog model. The current selection is unchanged; reopen /model to choose the provider, model and reasoning together.".into(),
        );
        true
    }

    pub(crate) fn open_catalog_provider(&mut self, provider: String) {
        match self.user_model_catalog() {
            Ok(snapshot) => self.open_catalog_models(snapshot, Some(&provider)),
            Err(error) => {
                self.add_error_message(format!("Cannot refresh model configuration: {error}"))
            }
        }
    }

    fn open_catalog_models(&mut self, snapshot: CatalogSnapshot, provider: Option<&str>) {
        let current_mode = self.effective_collaboration_mode().mode;
        let mut providers: Vec<_> = snapshot
            .models
            .iter()
            .map(|model| model.provider.clone())
            .collect();
        providers.sort_by_cached_key(|provider| provider_sort_key(&snapshot, provider));
        providers.dedup();
        let mut items = Vec::new();
        let mut initial_selected_idx = None;
        if provider.is_none() && providers.len() > 1 {
            for provider in providers {
                let selected = provider == self.config.model_provider_id;
                let label = snapshot
                    .providers
                    .get(&provider)
                    .map(|entry| entry.name.clone())
                    .unwrap_or_else(|| provider.clone());
                items.push(SelectionItem {
                    name: format!("Provider: {label} ({provider})"),
                    search_value: Some(format!("{label} {provider}")),
                    description: Some("Browse this provider's models".into()),
                    is_current: selected,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::OpenCatalogProvider {
                            provider: provider.clone(),
                        })
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                });
            }
        }
        let mut models: Vec<_> = snapshot
            .models
            .iter()
            .filter(|model| provider.is_none_or(|provider| provider == model.provider))
            .collect();
        models.sort_by_cached_key(|model| provider_sort_key(&snapshot, &model.provider));
        for model in models {
            let (status, disabled) = match static_check(&snapshot, &model.id) {
                Ok(check) => {
                    let label = match check.status {
                        ModelStatus::Ready => "ready",
                        ModelStatus::Unverified => "unverified",
                        ModelStatus::MissingCredentials => "missing credentials",
                        ModelStatus::Incompatible => "Responses incompatible",
                    };
                    let disabled = matches!(
                        check.status,
                        ModelStatus::MissingCredentials | ModelStatus::Incompatible
                    )
                    .then(|| check.issues.join("; "));
                    (label, disabled)
                }
                Err(_) => (
                    "invalid configuration",
                    Some("Invalid catalog entry".into()),
                ),
            };
            let id = model.id.clone();
            let discovery = if model.reasoning_efforts.is_none() {
                "; reasoning discovery pending"
            } else {
                ""
            };
            let is_current = self
                .catalog_selection_state
                .current_id
                .as_ref()
                .map_or_else(
                    || {
                        self.current_catalog_entry(&snapshot)
                            .ok()
                            .flatten()
                            .is_some_and(|entry| entry.id == model.id)
                    },
                    |id| {
                        id == &model.id
                            && self.config.model_provider_id == model.provider
                            && self.current_model() == model.model
                    },
                );
            if is_current && disabled.is_none() {
                initial_selected_idx = Some(items.len());
            }
            let provider_label = snapshot
                .providers
                .get(&model.provider)
                .map_or(model.provider.as_str(), |entry| entry.name.as_str());
            items.push(SelectionItem {
                name: format!("{} · {} [{}]", model.name, model.provider, model.id),
                search_value: Some(format!(
                    "{} {} {} {} {}",
                    model.name, provider_label, model.provider, model.id, model.model
                )),
                description: Some(format!("{} · {status}{discovery}", model.model)),
                is_current,
                disabled_reason: disabled,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenCatalogReasoning { id: id.clone() })
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }
        let header = self.model_menu_header(
            &format!("Select model · {} mode", current_mode.display_name()),
            "Choose a provider or model. This changes the current mode; type to search.",
        );
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(CATALOG_MODEL_VIEW),
            header,
            items,
            initial_selected_idx,
            is_searchable: true,
            search_placeholder: Some("Search provider, model, or catalog ID".into()),
            ..SelectionViewParams::picker()
        });
    }

    pub(crate) fn open_catalog_reasoning(&mut self, model: CatalogModel) {
        self.catalog_selection_state
            .discovered
            .insert(model.id.clone(), model.clone());
        let mode = self.effective_collaboration_mode().mode;
        let path = crate::project_model_memory::memory_path(
            self.config.xdcode_home.as_path(),
            &self.catalog_config_path(),
            self.config.cwd.as_path(),
        );
        let memory =
            crate::project_model_memory::ProjectModelMemory::load(&path).unwrap_or_default();
        let selected_effort = match memory.last_efforts.get(&model.id) {
            Some(None) => None,
            Some(Some(effort)) if model.supports_effort(effort) => Some(effort.clone()),
            _ => model
                .default_reasoning_effort
                .clone()
                .filter(|effort| model.supports_effort(effort)),
        };
        let is_current_entry = self
            .user_model_catalog()
            .ok()
            .and_then(|snapshot| {
                self.current_catalog_entry(&snapshot)
                    .ok()
                    .flatten()
                    .map(|entry| entry.id == model.id)
            })
            .unwrap_or_else(|| self.current_catalog_id() == Some(model.id.as_str()));
        let mut efforts = vec![None];
        efforts.extend(
            model
                .reasoning_efforts
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(Some),
        );
        let items = efforts
            .into_iter()
            .map(|effort| {
                let selection = CatalogModelSelection {
                    provider: model.provider.clone(),
                    catalog_id: model.id.clone(),
                    model: model.model.clone(),
                    effort: effort.clone(),
                    mode,
                };
                SelectionItem {
                    name: effort
                        .as_ref()
                        .map(|effort| effort.as_str().to_owned())
                        .unwrap_or_else(|| "Service default".into()),
                    description: if effort.is_none() {
                        Some("Do not send reasoning.effort".into())
                    } else {
                        None
                    },
                    is_current: if is_current_entry {
                        self.current_reasoning_effort() == effort
                    } else {
                        selected_effort == effort
                    },
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::SelectCatalogModel {
                            selection: selection.clone(),
                            persist: true,
                        })
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        let note = if model.reasoning_efforts.is_none() {
            "Discovery unavailable; service default is available. Use xdcode models refresh to retry."
        } else {
            "Select reasoning for this model and mode."
        };
        let header = self.model_menu_header(
            &format!("{} · {} mode", model.name, mode.display_name()),
            note,
        );
        self.bottom_pane.show_selection_view(SelectionViewParams {
            header,
            items,
            ..SelectionViewParams::picker()
        });
    }
}
