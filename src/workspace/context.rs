//! The resolved, immutable identity of one selected workspace.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

use super::{WorkspaceId, WorkspaceName, WorkspacePaths};

/// One workspace after identity and root resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContext {
    /// The workspace's immutable UUID.
    id: WorkspaceId,
    /// The workspace's canonical, human-facing name.
    name: WorkspaceName,
    /// The absolute lexically normalized workspace root.
    root: PathBuf,
    /// The user identity for this machine within the workspace.
    local_user_id: String,
    /// Machine-local runtime paths derived from the immutable workspace ID.
    paths: WorkspacePaths,
}

impl WorkspaceContext {
    /// Construct a resolved workspace context from explicit machine inputs.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceContextError`] when `root` is relative and
    /// `current_dir` is not absolute.
    pub fn new(
        home: &Path,
        id: WorkspaceId,
        name: WorkspaceName,
        root: &Path,
        local_user_id: impl Into<String>,
        current_dir: &Path,
    ) -> Result<Self, WorkspaceContextError> {
        Ok(Self {
            id,
            name,
            root: normalize_root(root, current_dir)?,
            local_user_id: local_user_id.into(),
            paths: WorkspacePaths::new(home, id),
        })
    }

    /// The workspace's immutable UUID.
    #[must_use]
    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    /// The workspace's canonical, human-facing name.
    #[must_use]
    pub fn name(&self) -> &WorkspaceName {
        &self.name
    }

    /// The absolute lexically normalized workspace root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The user identity for this machine within the workspace.
    #[must_use]
    pub fn local_user_id(&self) -> &str {
        &self.local_user_id
    }

    /// Machine-local runtime paths derived from the immutable workspace ID.
    #[must_use]
    pub fn paths(&self) -> &WorkspacePaths {
        &self.paths
    }
}

/// A workspace root cannot be made absolute from the supplied inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceContextError {
    /// A relative workspace root needs an absolute injected base directory.
    RelativeRootNeedsAbsoluteBase,
}

impl Display for WorkspaceContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RelativeRootNeedsAbsoluteBase => formatter
                .write_str("relative workspace roots require an absolute current directory"),
        }
    }
}

impl Error for WorkspaceContextError {}

/// Make `root` absolute against `current_dir` and remove lexical `.` and `..`
/// components without touching the filesystem.
///
/// # Errors
///
/// Returns [`WorkspaceContextError::RelativeRootNeedsAbsoluteBase`] when both
/// `root` and the injected `current_dir` are relative.
pub fn normalize_root(root: &Path, current_dir: &Path) -> Result<PathBuf, WorkspaceContextError> {
    if root.is_absolute() {
        return Ok(normalize_lexically(root));
    }
    if !current_dir.is_absolute() {
        return Err(WorkspaceContextError::RelativeRootNeedsAbsoluteBase);
    }
    Ok(normalize_lexically(&current_dir.join(root)))
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{normalize_root, WorkspaceContext, WorkspaceContextError};
    use crate::workspace::{WorkspaceId, WorkspaceName};

    #[test]
    fn normalize_root_resolves_relative_paths_against_the_supplied_directory() {
        assert_eq!(
            normalize_root(
                Path::new("notes/../personal/./projects"),
                Path::new("/workspaces")
            )
            .expect("absolute injected base"),
            PathBuf::from("/workspaces/personal/projects")
        );
    }

    #[test]
    fn normalize_root_does_not_require_the_path_to_exist() {
        assert_eq!(
            normalize_root(
                Path::new("/missing/./workspace/../personal"),
                Path::new("ignored")
            )
            .expect("absolute roots do not consult the base"),
            PathBuf::from("/missing/personal")
        );
    }

    #[test]
    fn normalize_root_rejects_a_relative_root_with_a_relative_base() {
        assert_eq!(
            normalize_root(Path::new("personal"), Path::new("workspaces")),
            Err(WorkspaceContextError::RelativeRootNeedsAbsoluteBase)
        );
    }

    #[test]
    fn context_construction_propagates_a_relative_base_error() {
        assert_eq!(
            WorkspaceContext::new(
                Path::new("/home/tester"),
                WorkspaceId::new(),
                WorkspaceName::parse("personal").expect("valid name"),
                Path::new("personal"),
                "tester",
                Path::new("workspaces"),
            ),
            Err(WorkspaceContextError::RelativeRootNeedsAbsoluteBase)
        );
    }
}
