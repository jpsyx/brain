//! Resolve one habits request to a validated workspace root.

use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use crate::workspace::{RegistryStore, WorkspaceId, WorkspaceManifest};

/// A workspace root resolved from an explicit request UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspace {
    workspace_id: WorkspaceId,
    root: PathBuf,
}

impl ResolvedWorkspace {
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    #[must_use]
    pub fn task_store_lock(&self) -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
        crate::workspace::WorkspacePaths::new(&home, self.workspace_id).task_store_lock()
    }
}

/// A request could not be safely mapped to one attached workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    status: u16,
    message: String,
}

impl ResolveError {
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }
}

impl Display for ResolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ResolveError {}

/// Resolve `workspace_id` from `url`, then validate the schema-v2 registry,
/// selected root, and portable manifest before returning a payload path.
pub fn resolve(store: &RegistryStore, url: &str) -> Result<ResolvedWorkspace, ResolveError> {
    let workspace_id = parse_workspace_id(url)?;
    let registry = RegistryStore::load_from(store.path()).map_err(|error| ResolveError {
        status: 500,
        message: format!("workspace registry unavailable: {error}"),
    })?;
    let record = registry
        .workspaces
        .values()
        .find(|record| record.workspace_id == workspace_id)
        .ok_or_else(|| ResolveError {
            status: 404,
            message: format!("workspace {workspace_id} is not attached"),
        })?;
    if !record.root.is_dir() {
        return Err(ResolveError {
            status: 503,
            message: format!("workspace root {} is unavailable", record.root.display()),
        });
    }
    let manifest =
        WorkspaceManifest::load(&record.root, env!("CARGO_PKG_VERSION")).map_err(|error| {
            ResolveError {
                status: 409,
                message: format!("workspace manifest is invalid: {error}"),
            }
        })?;
    if manifest.workspace_id() != workspace_id {
        return Err(ResolveError {
            status: 409,
            message: "workspace manifest identity does not match the request".to_owned(),
        });
    }
    Ok(ResolvedWorkspace {
        workspace_id,
        root: record.root.clone(),
    })
}

fn parse_workspace_id(url: &str) -> Result<WorkspaceId, ResolveError> {
    let query = url
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default();
    let mut values = query.split('&').filter_map(|field| {
        let (key, value) = field.split_once('=')?;
        (key == "workspace_id").then_some(value)
    });
    let Some(raw) = values.next().filter(|raw| !raw.is_empty()) else {
        return Err(ResolveError {
            status: 400,
            message: "workspace_id is required".to_owned(),
        });
    };
    if values.next().is_some() {
        return Err(ResolveError {
            status: 400,
            message: "workspace_id must appear exactly once".to_owned(),
        });
    }
    WorkspaceId::parse(raw).map_err(|_| ResolveError {
        status: 400,
        message: "workspace_id must be a UUID".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_workspace_id;

    const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

    #[test]
    fn request_uuid_is_required_exactly_once() {
        assert!(parse_workspace_id("/habits").is_err());
        assert!(parse_workspace_id("/habits?workspace_id=not-a-uuid").is_err());
        assert!(
            parse_workspace_id(&format!(
                "/habits?workspace_id={FAMILY_ID}&workspace_id={FAMILY_ID}"
            ))
            .is_err()
        );
        assert_eq!(
            parse_workspace_id(&format!("/habits?workspace_id={FAMILY_ID}"))
                .unwrap()
                .to_string(),
            FAMILY_ID
        );
    }
}
