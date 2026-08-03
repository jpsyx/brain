//! Pure readiness decisions for a selected workspace.

use std::error::Error;
use std::fmt::{Display, Formatter};

use super::{ManifestError, WorkspaceManifest, WorkspaceName, WorkspaceRecord};
use crate::users::{UserId, Users, UsersError};

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
    PortableUsers,
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
    let manifest = validated_manifest(record, manifest)?;

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

/// Decide readiness with the portable-membership invariant enabled.
pub fn readiness_action_with_users(
    canonical_name: &WorkspaceName,
    record: &WorkspaceRecord,
    manifest: Result<WorkspaceManifest, ManifestError>,
    users: Result<Users, UsersError>,
    interaction: InteractionMode,
) -> Result<ReadinessAction, ReadinessError> {
    let manifest = validated_manifest(record, manifest)?;

    let (users, legacy_compatible) = match users {
        Ok(users) if !users.users.is_empty() => (Some(users), false),
        Ok(_) => (None, false),
        Err(error) if error.is_missing_store() && !record.local_user_id.trim().is_empty() => {
            (None, true)
        }
        Err(error) if error.is_missing_store() => (None, false),
        Err(error) => return Err(ReadinessError::Users(error)),
    };
    let mut missing = Vec::new();
    if manifest.is_none() {
        missing.push(ReadinessField::Manifest);
    }
    let local_user = UserId::parse(record.local_user_id.trim()).ok();
    match users.as_ref() {
        None if !legacy_compatible => missing.push(ReadinessField::PortableUsers),
        None => {}
        Some(users) => {
            let valid_local = local_user
                .as_ref()
                .is_some_and(|local_user| users.user(local_user).is_some());
            if !valid_local {
                if interaction == InteractionMode::NonInteractive
                    && !record.local_user_id.trim().is_empty()
                {
                    return Err(ReadinessError::InvalidLocalUser {
                        canonical_name: canonical_name.to_string(),
                        user_id: record.local_user_id.clone(),
                    });
                }
                missing.push(ReadinessField::LocalUserId);
            }
        }
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

fn validated_manifest(
    record: &WorkspaceRecord,
    manifest: Result<WorkspaceManifest, ManifestError>,
) -> Result<Option<WorkspaceManifest>, ReadinessError> {
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
    Ok(manifest)
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
    Users(UsersError),
    WorkspaceIdMismatch {
        registry: String,
        manifest: String,
    },
    InvalidLocalUser {
        canonical_name: String,
        user_id: String,
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
            Self::Users(error) => Display::fmt(error, formatter),
            Self::WorkspaceIdMismatch { registry, manifest } => write!(
                formatter,
                "workspace manifest UUID {manifest} does not match registry UUID {registry}"
            ),
            Self::InvalidLocalUser {
                canonical_name,
                user_id,
            } => write!(
                formatter,
                "local user {user_id} is not a portable member\n  brain user local <USER_ID> -b {canonical_name}"
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
                        ReadinessField::PortableUsers => write!(
                            formatter,
                            "\n  brain user add -b {canonical_name} --id <USER_ID> --name <DISPLAY_NAME>\n  brain user local <USER_ID> -b {canonical_name}"
                        )?,
                        ReadinessField::LocalUserId => write!(
                            formatter,
                            "\n  brain user local <USER_ID> -b {canonical_name}"
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
            Self::Users(error) => Some(error),
            Self::WorkspaceIdMismatch { .. }
            | Self::InvalidLocalUser { .. }
            | Self::Incomplete { .. } => None,
        }
    }
}
