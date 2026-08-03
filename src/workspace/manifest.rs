//! Portable workspace identity manifest.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::WorkspaceId;

/// The only portable manifest schema this release accepts.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Portable identity stored at `<workspace-root>/.config/workspace.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceManifest {
    schema_version: u32,
    workspace_id: WorkspaceId,
    receiver_ingress_id: WorkspaceId,
    minimum_brain_version: String,
}

impl WorkspaceManifest {
    /// Build a manifest for a newly created workspace.
    #[must_use]
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            workspace_id,
            receiver_ingress_id: WorkspaceId::new(),
            minimum_brain_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    /// Parse and validate manifest bytes against one Brain version.
    pub fn parse(bytes: &[u8], brain_version: &str) -> Result<Self, ManifestError> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|error| ManifestError::InvalidJson {
                message: error.to_string(),
            })?;
        manifest.validate(brain_version)?;
        Ok(manifest)
    }

    /// Load and validate a manifest from a workspace root.
    pub fn load(root: &Path, brain_version: &str) -> Result<Self, ManifestError> {
        let path = Self::path(root);
        let bytes = std::fs::read(&path).map_err(|error| ManifestError::Io {
            operation: "read workspace manifest",
            path: path.clone(),
            kind: error.kind(),
            message: error.to_string(),
        })?;
        Self::parse(&bytes, brain_version)
    }

    /// Create this manifest without replacing an existing portable identity.
    pub fn write_new(&self, root: &Path) -> Result<(), ManifestError> {
        self.write_new_with(
            root,
            |temporary, bytes| {
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(temporary)?;
                file.write_all(bytes)?;
                file.sync_all()
            },
            |temporary, path| std::fs::hard_link(temporary, path),
        )
    }

    fn write_new_with(
        &self,
        root: &Path,
        write_and_sync: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
        publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<(), ManifestError> {
        let path = Self::path(root);
        let parent = path
            .parent()
            .expect("workspace manifest always has a .config parent");
        std::fs::create_dir_all(parent).map_err(|error| ManifestError::Io {
            operation: "create workspace config directory",
            path: parent.to_path_buf(),
            kind: error.kind(),
            message: error.to_string(),
        })?;
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|error| ManifestError::InvalidJson {
                message: error.to_string(),
            })?;
        bytes.push(b'\n');
        let temporary = parent.join(format!(".workspace.json.{}.tmp", WorkspaceId::new()));
        if let Err(error) = write_and_sync(&temporary, &bytes) {
            let _ = std::fs::remove_file(&temporary);
            return Err(ManifestError::Io {
                operation: "write temporary workspace manifest",
                path: temporary,
                kind: error.kind(),
                message: error.to_string(),
            });
        }
        let result = publish(&temporary, &path).map_err(|error| ManifestError::Io {
            operation: "publish workspace manifest",
            path: path.clone(),
            kind: error.kind(),
            message: error.to_string(),
        });
        let _ = std::fs::remove_file(&temporary);
        result
    }

    /// The manifest path below a workspace root.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(".config").join("workspace.json")
    }

    /// Stable workspace UUID.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    /// Stable receiver ingress UUID.
    #[must_use]
    pub const fn receiver_ingress_id(&self) -> WorkspaceId {
        self.receiver_ingress_id
    }

    /// Minimum compatible Brain version recorded by this workspace.
    #[must_use]
    pub fn minimum_brain_version(&self) -> &str {
        &self.minimum_brain_version
    }

    fn validate(&self, brain_version: &str) -> Result<(), ManifestError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchema {
                found: self.schema_version,
                supported: MANIFEST_SCHEMA_VERSION,
            });
        }
        let minimum = parse_version(&self.minimum_brain_version).ok_or_else(|| {
            ManifestError::InvalidMinimumBrainVersion {
                value: self.minimum_brain_version.clone(),
            }
        })?;
        let current = parse_version(brain_version).ok_or_else(|| {
            ManifestError::InvalidCurrentBrainVersion {
                value: brain_version.to_owned(),
            }
        })?;
        if current < minimum {
            return Err(ManifestError::IncompatibleBrainVersion {
                current: brain_version.to_owned(),
                minimum: self.minimum_brain_version.clone(),
            });
        }
        Ok(())
    }
}

fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split_once('-').map_or(version, |(core, _)| core);
    let mut parts = core.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(parsed)
}

/// A portable workspace manifest could not be loaded or validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    Io {
        operation: &'static str,
        path: PathBuf,
        kind: std::io::ErrorKind,
        message: String,
    },
    InvalidJson {
        message: String,
    },
    UnsupportedSchema {
        found: u32,
        supported: u32,
    },
    InvalidMinimumBrainVersion {
        value: String,
    },
    InvalidCurrentBrainVersion {
        value: String,
    },
    IncompatibleBrainVersion {
        current: String,
        minimum: String,
    },
}

impl Display for ManifestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                message,
                ..
            } => {
                write!(formatter, "{operation} {}: {message}", path.display())
            }
            Self::InvalidJson { message } => {
                write!(formatter, "invalid workspace manifest: {message}")
            }
            Self::UnsupportedSchema { found, supported } => write!(
                formatter,
                "workspace manifest schema {found} is unsupported; this Brain supports schema {supported}"
            ),
            Self::InvalidMinimumBrainVersion { value } => write!(
                formatter,
                "workspace manifest has invalid minimum Brain version {value}"
            ),
            Self::InvalidCurrentBrainVersion { value } => {
                write!(formatter, "Brain has invalid build version {value}")
            }
            Self::IncompatibleBrainVersion { current, minimum } => write!(
                formatter,
                "workspace requires Brain {minimum} or newer; this is Brain {current}"
            ),
        }
    }
}

impl Error for ManifestError {}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::WorkspaceManifest;
    use crate::workspace::WorkspaceId;

    fn manifest() -> WorkspaceManifest {
        WorkspaceManifest::new(WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap())
    }

    fn create_temp(path: &std::path::Path) -> std::fs::File {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
    }

    #[test]
    fn injected_manifest_write_failure_never_publishes_a_partial_destination() {
        let root = tempfile::tempdir().unwrap();

        let error = manifest()
            .write_new_with(
                root.path(),
                |temporary, bytes| {
                    let mut file = create_temp(temporary);
                    file.write_all(&bytes[..bytes.len() / 2])?;
                    Err(std::io::Error::other("injected write failure"))
                },
                |_, _| unreachable!("a failed write cannot publish"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("injected write failure"));
        assert!(!WorkspaceManifest::path(root.path()).exists());
    }

    #[test]
    fn injected_manifest_sync_failure_never_publishes_an_unsynced_destination() {
        let root = tempfile::tempdir().unwrap();

        let error = manifest()
            .write_new_with(
                root.path(),
                |temporary, bytes| {
                    let mut file = create_temp(temporary);
                    file.write_all(bytes)?;
                    Err(std::io::Error::other("injected sync failure"))
                },
                |_, _| unreachable!("a failed sync cannot publish"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("injected sync failure"));
        assert!(!WorkspaceManifest::path(root.path()).exists());
    }

    #[test]
    fn injected_manifest_publication_failure_leaves_the_destination_absent() {
        let root = tempfile::tempdir().unwrap();

        let error = manifest()
            .write_new_with(
                root.path(),
                |temporary, bytes| {
                    let mut file = create_temp(temporary);
                    file.write_all(bytes)?;
                    file.sync_all()
                },
                |_, _| Err(std::io::Error::other("injected publication failure")),
            )
            .unwrap_err();

        assert!(error.to_string().contains("injected publication failure"));
        assert!(!WorkspaceManifest::path(root.path()).exists());
    }
}
