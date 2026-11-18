//! Modal pickers for models, reasoning effort, `@` file mentions and `$` skills.

use orca_harness_model_providers::openrouter::ModelInfo;
use orca_harness_model_providers::SupportedEfforts;

use crate::tui::components::picker::ListPicker;

/// The interactive model selector: the fetched catalog, a live-typed
/// filter, and the standard shared picker state for the filtered view.
pub(crate) struct ModelPicker {
    pub(crate) subagent: Option<(String, crate::Provider)>,
    pub(crate) models: Vec<ModelInfo>,
    pub(crate) filter: String,
    pub(crate) picker: ListPicker,
}

/// The picker to open when a model-catalog request completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ModelPickerTarget {
    Models {
        filter: String,
    },
    ActiveModelEffort,
    Subagent {
        tier: String,
        provider: crate::Provider,
    },
}

/// Provider-advertised reasoning efforts for the active or newly chosen model.
pub(crate) struct EffortPicker {
    pub(crate) model_id: String,
    pub(crate) context_window: Option<u64>,
    pub(crate) efforts: Vec<String>,
    pub(crate) picker: ListPicker,
}

/// Workspace-relative file and directory inserted into the composer by
/// the `@` mention picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocationEntry {
    pub(crate) path: String,
    pub(crate) directory: bool,
}

pub(crate) struct LocationPicker {
    pub(crate) entries: Vec<LocationEntry>,
    pub(crate) query: String,
    /// Character offset of the `@` that opened this picker.
    pub(crate) token_start: usize,
    pub(crate) picker: ListPicker,
}

/// An enabled, loaded skill inserted into the composer by the `$` picker.
pub(crate) struct SkillMentionPicker {
    pub(crate) entries: Vec<crate::skills::SkillEntry>,
    pub(crate) query: String,
    /// Character offset of the `$` that opened this picker.
    pub(crate) token_start: usize,
    pub(crate) picker: ListPicker,
}

impl LocationPicker {
    pub(crate) fn filtered(&self) -> Vec<&LocationEntry> {
        let needle = self.query.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| needle.is_empty() || entry.path.to_lowercase().contains(&needle))
            .collect()
    }

    pub(crate) fn selected(&self) -> Option<LocationEntry> {
        self.filtered().get(self.picker.index()).cloned().cloned()
    }

    pub(crate) fn sync_len(&mut self) {
        let len = self.filtered().len();
        self.picker.set_len(len);
    }
}

impl SkillMentionPicker {
    pub(crate) fn filtered(&self) -> Vec<&crate::skills::SkillEntry> {
        let needle = self.query.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| needle.is_empty() || entry.name.to_lowercase().contains(&needle))
            .collect()
    }

    pub(crate) fn selected(&self) -> Option<String> {
        self.filtered()
            .get(self.picker.index())
            .map(|entry| entry.name.clone())
    }

    pub(crate) fn sync_len(&mut self) {
        let len = self.filtered().len();
        self.picker.set_len(len);
    }
}

impl ModelPicker {
    pub(crate) fn new(models: Vec<ModelInfo>, filter: String) -> Self {
        let needle = filter.to_lowercase();
        let len = models
            .iter()
            .filter(|model| needle.is_empty() || model.id.to_lowercase().contains(&needle))
            .count();
        Self {
            subagent: None,
            models,
            filter,
            picker: ListPicker::new(len),
        }
    }

    pub(crate) fn filtered(&self) -> Vec<&ModelInfo> {
        let needle = self.filter.to_lowercase();
        self.models
            .iter()
            .filter(|m| needle.is_empty() || m.id.to_lowercase().contains(&needle))
            .collect()
    }

    /// The provider metadata under the cursor, if any model matches.
    pub(crate) fn selected_model(&self) -> Option<ModelInfo> {
        let filtered = self.filtered();
        filtered.get(self.picker.index()).cloned().cloned()
    }

    pub(crate) fn reset_filtered_selection(&mut self) {
        self.picker = ListPicker::new(self.filtered().len());
    }
}

impl EffortPicker {
    pub(crate) fn new(model: ModelInfo) -> Option<Self> {
        let reasoning = model.reasoning?;
        let SupportedEfforts::Listed(efforts) = reasoning.supported_efforts? else {
            return None;
        };
        if efforts.is_empty() {
            return None;
        }
        let selected = reasoning
            .default_effort
            .as_ref()
            .and_then(|default| efforts.iter().position(|effort| effort == default))
            .unwrap_or(0);
        Some(Self {
            model_id: model.id,
            context_window: model.context_length,
            picker: ListPicker::with_selected(efforts.len(), selected),
            efforts,
        })
    }

    pub(crate) fn selected(&self) -> Option<String> {
        self.efforts.get(self.picker.index()).cloned()
    }
}
