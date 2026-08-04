//! Immutable task identity, separate from mutable human-facing display IDs.

use std::fmt::{Display, Formatter};

use uuid::Uuid;

use crate::workspace::WorkspaceId;

/// Which portable task CSV owns a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvKind {
    Tasks,
    Habits,
}

impl CsvKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tasks => "tasks",
            Self::Habits => "habits",
        }
    }
}

/// The immutable identity of one task or habit occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskUuid(Uuid);

impl TaskUuid {
    /// Create an identity for a new row.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a persisted task UUID.
    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }
}

impl Default for TaskUuid {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for TaskUuid {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Derive a stable UUID for one row that predates immutable task identity.
#[must_use]
pub fn legacy_task_uuid(
    workspace_id: WorkspaceId,
    csv_kind: CsvKind,
    legacy_task_id: &str,
) -> TaskUuid {
    let input = format!("{workspace_id}:{}:{legacy_task_id}", csv_kind.as_str());
    TaskUuid(Uuid::new_v5(&Uuid::NAMESPACE_OID, input.as_bytes()))
}
