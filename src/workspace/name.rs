//! Canonical, human-facing workspace names.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

/// A validated, canonical workspace name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkspaceName(String);

impl WorkspaceName {
    /// Parse and canonicalize a workspace name.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceNameError`] when the trimmed name is empty or does
    /// not match `[a-z0-9][a-z0-9_-]*` after lower-casing.
    pub fn parse(value: &str) -> Result<Self, WorkspaceNameError> {
        let canonical = value.trim().to_ascii_lowercase();
        if canonical.is_empty() {
            return Err(WorkspaceNameError::Empty);
        }

        let mut bytes = canonical.bytes();
        if !bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(WorkspaceNameError::Invalid);
        }
        if !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        }) {
            return Err(WorkspaceNameError::Invalid);
        }

        Ok(Self(canonical))
    }

    /// Derive a canonical name from the final component of a root path.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceNameError`] when the root has no valid final path
    /// component or that component is not a canonical workspace name.
    pub fn from_root(root: &Path) -> Result<Self, WorkspaceNameError> {
        root.file_name()
            .and_then(|name| name.to_str())
            .ok_or(WorkspaceNameError::Invalid)
            .and_then(Self::parse)
    }

    /// The canonical workspace name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A workspace name is absent or is not a valid canonical slug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceNameError {
    /// The value contains no non-whitespace characters.
    Empty,
    /// The value does not match `[a-z0-9][a-z0-9_-]*`.
    Invalid,
}

impl Display for WorkspaceNameError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("workspace name cannot be empty"),
            Self::Invalid => formatter.write_str("workspace name must match [a-z0-9][a-z0-9_-]*"),
        }
    }
}

impl Error for WorkspaceNameError {}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::WorkspaceName;

    #[test]
    fn from_root_uses_its_final_component() {
        let name = WorkspaceName::from_root(Path::new("/workspaces/Family_Notes"))
            .expect("valid workspace root name");

        assert_eq!(name.as_str(), "family_notes");
    }

    #[test]
    fn from_root_rejects_a_root_without_a_name() {
        assert!(WorkspaceName::from_root(Path::new("/")).is_err());
    }
}
