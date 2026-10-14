//! Explicit deletion policies and serializable domain failures.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Dependents {
    #[default]
    Reject,
    Unlink,
    Rewire,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum LinkedIssue {
    #[default]
    Configured,
    Keep,
    Cancel,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Options {
    pub habit: bool,
    pub dependents: Dependents,
    pub linked_issue: LinkedIssue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

impl Problem {
    pub(crate) fn new(code: &str, message: &str, details: serde_json::Value) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Identity {
    #[serde(with = "csv_kind")]
    pub csv: crate::tasks::identity::CsvKind,
    pub task_id: String,
    #[serde(with = "task_uuid")]
    pub task_uuid: crate::tasks::identity::TaskUuid,
    pub task_name: String,
}

/// Integration semantics only. Credentials have no representation here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IntegrationSnapshot {
    pub adapter: Option<AdapterSnapshot>,
    pub policy: Option<IntegrationPolicy>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AdapterSnapshot {
    pub argv: Vec<String>,
    pub environment_names: std::collections::BTreeSet<String>,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileEdit {
    pub relative: std::path::PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExternalAction {
    pub adapter: String,
    pub issue: String,
    pub action_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemovalPlan {
    pub fingerprint: String,
    pub targets: Vec<Identity>,
    pub options: Options,
    pub edits: Vec<FileEdit>,
    pub external: Vec<ExternalAction>,
    pub successors: std::collections::BTreeMap<String, String>,
    pub blockers: Vec<Problem>,
    pub warnings: Vec<Problem>,
}

mod csv_kind {
    use crate::tasks::identity::CsvKind;
    use serde::{Deserialize, Deserializer, Serializer};
    // Serde requires the field serializer to accept a reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn serialize<S: Serializer>(
        value: &CsvKind,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value.as_str())
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<CsvKind, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "tasks" => Ok(CsvKind::Tasks),
            "habits" => Ok(CsvKind::Habits),
            _ => Err(serde::de::Error::custom("invalid CSV kind")),
        }
    }
}

mod task_uuid {
    use crate::tasks::identity::TaskUuid;
    use serde::{Deserialize, Deserializer, Serializer};
    pub(super) fn serialize<S: Serializer>(
        value: &TaskUuid,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<TaskUuid, D::Error> {
        TaskUuid::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Validated portable policy. Machine adapter availability is supplied separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationPolicy {
    pub adapter: String,
    pub link_field: String,
    pub on_remove: OnRemove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnRemove {
    Keep,
    Cancel,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Problem {}
