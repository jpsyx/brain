//! Pure readiness decisions for a selected workspace.

use std::error::Error;
use std::fmt::{Display, Formatter};

use super::{ManifestError, WorkspaceManifest, WorkspaceName, WorkspaceRecord};

/// Whether a command may interact with a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionMode {
    Interactive,
    NonInteractive,
    Internal,
}

/// A required workspace field that guided setup can repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessField {
    Manifest,
    LocalUserId,
}

/// The next action after evaluating selected workspace readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadinessAction {
    Ready(WorkspaceManifest),
    Prompt(Vec<ReadinessField>),
}

/// Decide whether a selected workspace is ready for an ordinary command.
pub fn readiness_action(
    canonical_name: &WorkspaceName,
    record: &WorkspaceRecord,
    manifest: Result<WorkspaceManifest, ManifestError>,
    interaction: InteractionMode,
) -> Result<ReadinessAction, ReadinessError> {
    let manifest = match manifest {
        Ok(manifest) => Some(manifest),
        Err(error) if is_missing_manifest(&error) => None,
        Err(error) => return Err(ReadinessError::Manifest(error)),
    };
    if let Some(manifest) = manifest.as_ref()
        && manifest.workspace_id() != record.workspace_id
    {
        return Err(ReadinessError::WorkspaceIdMismatch {
            registry: record.workspace_id.to_string(),
            manifest: manifest.workspace_id().to_string(),
        });
    }

    let mut missing = Vec::new();
    if manifest.is_none() {
        missing.push(ReadinessField::Manifest);
    }
    if record.local_user_id.trim().is_empty() {
        missing.push(ReadinessField::LocalUserId);
    }
    if missing.is_empty() {
        return Ok(ReadinessAction::Ready(
            manifest.expect("manifest exists when no readiness field is missing"),
        ));
    }
    if interaction == InteractionMode::Interactive {
        return Ok(ReadinessAction::Prompt(missing));
    }
    Err(ReadinessError::Incomplete {
        canonical_name: canonical_name.to_string(),
        missing,
        internal: interaction == InteractionMode::Internal,
    })
}

fn is_missing_manifest(error: &ManifestError) -> bool {
    matches!(
        error,
        ManifestError::Io {
            kind: std::io::ErrorKind::NotFound,
            ..
        }
    )
}

/// A selected workspace cannot safely serve the requested command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadinessError {
    Manifest(ManifestError),
    WorkspaceIdMismatch {
        registry: String,
        manifest: String,
    },
    Incomplete {
        canonical_name: String,
        missing: Vec<ReadinessField>,
        internal: bool,
    },
}

impl Display for ReadinessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(error) => Display::fmt(error, formatter),
            Self::WorkspaceIdMismatch { registry, manifest } => write!(
                formatter,
                "workspace manifest UUID {manifest} does not match registry UUID {registry}"
            ),
            Self::Incomplete {
                canonical_name,
                missing,
                internal,
            } => {
                if *internal {
                    write!(formatter, "workspace {canonical_name} is unavailable")?;
                } else {
                    write!(formatter, "workspace {canonical_name} needs setup")?;
                }
                for field in missing {
                    match field {
                        ReadinessField::Manifest => write!(
                            formatter,
                            "\n  brain workspace repair -b {canonical_name} --manifest"
                        )?,
                        ReadinessField::LocalUserId => write!(
                            formatter,
                            "\n  brain workspace repair -b {canonical_name} --local-user-id <USER_ID>"
                        )?,
                    }
                }
                Ok(())
            }
        }
    }
}

impl Error for ReadinessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Manifest(error) => Some(error),
            Self::WorkspaceIdMismatch { .. } | Self::Incomplete { .. } => None,
        }
    }
}
