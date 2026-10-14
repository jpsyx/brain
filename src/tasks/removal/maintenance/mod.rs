//! Frozen, typed maintenance intent composed by the ordinary removal engine.
mod history;
mod preview;
mod validate;
use super::model::{Dependents, Identity, LinkedIssue, Options};
pub(super) use history::{enrich, project_edits};
pub(crate) use preview::{MaintenanceKind, preview_maintenance};
use serde::{Deserialize, Serialize};
pub(super) use validate::validate;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum MaintenanceIntent {
    BacklogPurge {
        cutoff: String,
    },
    BacklogDedupe {
        replacements: Vec<DuplicateReplacement>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DuplicateReplacement {
    pub duplicate: Identity,
    pub survivor: Identity,
}
pub(super) fn options() -> Options {
    Options {
        habit: false,
        dependents: Dependents::Rewire,
        linked_issue: LinkedIssue::Keep,
    }
}
pub(super) fn replacements(
    intent: Option<&MaintenanceIntent>,
) -> std::collections::BTreeMap<String, String> {
    match intent {
        Some(MaintenanceIntent::BacklogDedupe { replacements }) => replacements
            .iter()
            .map(|pair| {
                (
                    pair.duplicate.task_id.clone(),
                    pair.survivor.task_id.clone(),
                )
            })
            .collect(),
        _ => std::collections::BTreeMap::new(),
    }
}
