//! Durable operation progress, separate from immutable local commit evidence.
use super::model::{Identity, Problem, RemovalPlan};
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
    pub external: Vec<serde_json::Value>,
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
    pub external: Vec<serde_json::Value>,
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
