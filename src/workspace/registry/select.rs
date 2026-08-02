//! Borrowed canonical workspace selection.

use super::{MachineRegistry, RegistryError, WorkspaceRecord, validate_registry};
use crate::workspace::WorkspaceName;

/// A selected canonical name paired with its borrowed, siloed record.
#[derive(Debug, Clone, Copy)]
pub struct SelectedWorkspace<'a> {
    canonical_name: &'a WorkspaceName,
    record: &'a WorkspaceRecord,
}

impl<'a> SelectedWorkspace<'a> {
    /// The canonical name corresponding to the requested selector.
    #[must_use]
    pub fn canonical_name(self) -> &'a WorkspaceName {
        self.canonical_name
    }

    /// The exact selected record, without merged environment data.
    #[must_use]
    pub fn record(self) -> &'a WorkspaceRecord {
        self.record
    }
}

impl MachineRegistry {
    /// Select by canonical name or alias, or use the canonical default.
    pub fn select(&self, selector: Option<&str>) -> Result<SelectedWorkspace<'_>, RegistryError> {
        validate_registry(self)?;
        let requested = selector.unwrap_or(self.default_workspace.as_str());
        let folded = requested.trim().to_ascii_lowercase();

        if let Some((canonical_name, record)) = self
            .workspaces
            .iter()
            .find(|(canonical_name, _)| canonical_name.as_str() == folded)
        {
            return Ok(SelectedWorkspace {
                canonical_name,
                record,
            });
        }

        self.workspaces
            .iter()
            .find(|(_, record)| {
                record
                    .aliases
                    .iter()
                    .any(|alias| alias.as_str().eq_ignore_ascii_case(&folded))
            })
            .map(|(canonical_name, record)| SelectedWorkspace {
                canonical_name,
                record,
            })
            .ok_or_else(|| RegistryError::UnknownSelector {
                selector: requested.to_owned(),
            })
    }
}
