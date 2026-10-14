//! Durable operation progress, separate from immutable local commit evidence.
use super::model::{ExternalAction, Identity, Problem, RemovalPlan};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Prepared,
    ExternalPending,
    ExternalComplete,
    LocalCommitted,
    Complete,
    NeedsReconfirmation,
    Abandoned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub operation_id: String,
    pub committed: bool,
    pub removed: Vec<Identity>,
    /// Sorted, deduplicated workspace-relative canonical files changed by the commit.
    pub updated: Vec<String>,
    pub external: Vec<ExternalProgress>,
    pub warnings: Vec<Problem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_field_names)] // Stable serialized protocol field.
pub(crate) struct Operation {
    pub schema_version: u32,
    pub operation_id: String,
    pub workspace_uuid: String,
    pub actor_id: String,
    pub date: String,
    pub fingerprint: String,
    pub canonical_inputs: serde_json::Value,
    pub plan: RemovalPlan,
    pub phase: Phase,
    pub external: Vec<ExternalProgress>,
    pub receipt: Option<Receipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommitRecord {
    pub schema_version: u32,
    pub workspace_uuid: String,
    pub fingerprint: String,
    pub receipt: Receipt,
}

mod storage;
pub(super) use storage::{OperationOwner, load, load_commit};
pub(crate) use storage::{persist_commit, validate_commit};

/// Unknown is persisted before invocation and survives every ambiguous failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExternalState {
    Pending,
    Unknown,
    Succeeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalProgress {
    pub action_id: String,
    pub adapter: String,
    pub issue: String,
    pub state: ExternalState,
}

impl From<&ExternalAction> for ExternalProgress {
    fn from(action: &ExternalAction) -> Self {
        Self {
            action_id: action.action_id.clone(),
            adapter: action.adapter.clone(),
            issue: action.issue.clone(),
            state: ExternalState::Pending,
        }
    }
}

#[cfg(test)]
mod tests;
