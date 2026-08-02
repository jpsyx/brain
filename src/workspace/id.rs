//! Stable workspace UUIDs.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// The immutable identifier for one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(Uuid);

impl WorkspaceId {
    /// Create a fresh workspace identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a persisted workspace UUID.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceIdError`] when `value` is not a UUID.
    pub fn parse(value: &str) -> Result<Self, WorkspaceIdError> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| WorkspaceIdError)
    }
}

impl Default for WorkspaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for WorkspaceId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for WorkspaceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for WorkspaceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// A value could not be parsed as a workspace UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceIdError;

impl Display for WorkspaceIdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("workspace ID must be a UUID")
    }
}

impl Error for WorkspaceIdError {}

#[cfg(test)]
mod tests {
    use super::WorkspaceId;

    #[test]
    fn parse_rejects_non_uuid_values() {
        assert!(WorkspaceId::parse("not-a-uuid").is_err());
    }

    #[test]
    fn new_creates_distinct_workspace_ids() {
        assert_ne!(WorkspaceId::new(), WorkspaceId::new());
    }

    #[test]
    fn default_creates_a_workspace_id() {
        assert_ne!(WorkspaceId::default(), WorkspaceId::default());
    }
}
