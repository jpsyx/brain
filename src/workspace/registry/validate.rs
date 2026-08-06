//! Pure whole-registry validation.

use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use super::{MachineRegistry, REGISTRY_SCHEMA_VERSION};
use crate::workspace::ManifestError;
use crate::workspace::{WorkspaceId, WorkspaceName, context::normalize_root};

/// The storage operation that failed at a registry boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryOperation {
    AcquireTransactionLock,
    WriteTransactionLock,
    ReadRegistry,
    ParseRegistry,
    CreateLegacyBackup,
    WriteLegacyBackup,
    SyncLegacyBackup,
    SerializeRegistry,
    CreateDirectory,
    CreateTemporary,
    WriteTemporary,
    WritePortableConfig,
    SyncTemporary,
    ReplaceRegistry,
}

impl Display for RegistryOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AcquireTransactionLock => "acquire workspace registry transaction lock",
            Self::WriteTransactionLock => "write workspace registry transaction lock",
            Self::ReadRegistry => "read workspace registry",
            Self::ParseRegistry => "parse workspace registry JSON",
            Self::CreateLegacyBackup => "create legacy environment backup",
            Self::WriteLegacyBackup => "write legacy environment backup",
            Self::SyncLegacyBackup => "sync legacy environment backup",
            Self::SerializeRegistry => "serialize workspace registry",
            Self::CreateDirectory => "create workspace registry directory",
            Self::CreateTemporary => "create temporary workspace registry",
            Self::WriteTemporary => "write temporary workspace registry",
            Self::WritePortableConfig => "write portable workspace config",
            Self::SyncTemporary => "sync temporary workspace registry",
            Self::ReplaceRegistry => "replace workspace registry",
        })
    }
}

/// A registry violates its schema, uniqueness, or storage contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// Another process held the transaction lock past the bounded wait.
    LockTimeout {
        path: PathBuf,
        owner_pid: Option<u32>,
        waited_millis: u64,
    },
    /// The persisted schema is not the exact supported version.
    UnsupportedSchemaVersion { found: u32 },
    /// A registry must contain at least one workspace.
    EmptyRegistry,
    /// The default does not name a canonical record.
    MissingDefault { default_workspace: WorkspaceName },
    /// A case-folded canonical name or alias selects more than one record.
    DuplicateSelector {
        selector: String,
        first_workspace: WorkspaceName,
        second_workspace: WorkspaceName,
    },
    /// Two records carry the same immutable UUID.
    DuplicateWorkspaceId { workspace_id: WorkspaceId },
    /// A stored root is not absolute.
    RelativeRoot {
        canonical_name: WorkspaceName,
        root: PathBuf,
    },
    /// Two normalized roots are equal or one contains the other.
    OverlappingRoots { first: PathBuf, second: PathBuf },
    /// No canonical name or alias matched a requested selector.
    UnknownSelector { selector: String },
    /// A canonical workspace does not exist.
    UnknownWorkspace { canonical_name: String },
    /// A selected canonical name was replaced before mutation.
    WorkspaceIdentityChanged {
        canonical_name: WorkspaceName,
        expected: WorkspaceId,
        found: WorkspaceId,
    },
    /// A canonical workspace already exists.
    WorkspaceAlreadyExists { canonical_name: WorkspaceName },
    /// The requested alias already belongs to the same canonical workspace.
    AliasAlreadyExists {
        canonical_name: WorkspaceName,
        alias: WorkspaceName,
    },
    /// An alias does not exist on the requested record.
    UnknownAlias { alias: String },
    /// Registry JSON could not be read or written.
    Json {
        operation: RegistryOperation,
        path: PathBuf,
        message: String,
    },
    /// Registry storage failed.
    Io {
        operation: RegistryOperation,
        path: PathBuf,
        related_path: Option<PathBuf>,
        kind: std::io::ErrorKind,
        message: String,
    },
    /// Portable manifest creation or validation failed during migration.
    Manifest(ManifestError),
}

impl Display for RegistryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LockTimeout {
                path,
                owner_pid,
                waited_millis,
            } => {
                write!(
                    formatter,
                    "workspace registry transaction lock at {} remained held",
                    path.display()
                )?;
                if let Some(owner_pid) = owner_pid {
                    write!(formatter, " by PID {owner_pid}")?;
                }
                write!(
                    formatter,
                    " after {waited_millis}ms; retry after that process finishes. The operating system releases this lock if its owner exits; a persistent timeout indicates a live writer or a filesystem locking problem"
                )
            }
            Self::UnsupportedSchemaVersion { found } => write!(
                formatter,
                "unsupported workspace registry schema {found}; expected {REGISTRY_SCHEMA_VERSION}"
            ),
            Self::EmptyRegistry => formatter.write_str("workspace registry cannot be empty"),
            Self::MissingDefault { default_workspace } => {
                write!(
                    formatter,
                    "default workspace {default_workspace} does not exist"
                )
            }
            Self::DuplicateSelector { selector, .. } => {
                write!(formatter, "workspace selector {selector} is not unique")
            }
            Self::DuplicateWorkspaceId { workspace_id } => {
                write!(formatter, "workspace ID {workspace_id} is not unique")
            }
            Self::RelativeRoot {
                canonical_name,
                root,
            } => write!(
                formatter,
                "workspace {canonical_name} root {} is not absolute",
                root.display()
            ),
            Self::OverlappingRoots { first, second } => write!(
                formatter,
                "workspace roots {} and {} overlap",
                first.display(),
                second.display()
            ),
            Self::UnknownSelector { selector } => {
                write!(formatter, "unknown workspace selector {selector}")
            }
            Self::UnknownWorkspace { canonical_name } => {
                write!(formatter, "unknown canonical workspace {canonical_name}")
            }
            Self::WorkspaceIdentityChanged {
                canonical_name,
                expected,
                found,
            } => write!(
                formatter,
                "workspace {canonical_name} identity changed from {expected} to {found}"
            ),
            Self::WorkspaceAlreadyExists { canonical_name } => {
                write!(formatter, "workspace {canonical_name} already exists")
            }
            Self::AliasAlreadyExists {
                canonical_name,
                alias,
            } => write!(
                formatter,
                "workspace {canonical_name} already has alias {alias}"
            ),
            Self::UnknownAlias { alias } => write!(formatter, "unknown workspace alias {alias}"),
            Self::Json {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "failed to {operation} at {}: {message}",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                related_path,
                message,
                ..
            } => {
                write!(formatter, "failed to {operation} at {}", path.display())?;
                if let Some(related_path) = related_path {
                    write!(formatter, " using {}", related_path.display())?;
                }
                write!(formatter, ": {message}")
            }
            Self::Manifest(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for RegistryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Manifest(error) => Some(error),
            _ => None,
        }
    }
}

/// Validate every invariant that makes registry selection unambiguous and safe.
pub fn validate_registry(registry: &MachineRegistry) -> Result<(), RegistryError> {
    if registry.schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(RegistryError::UnsupportedSchemaVersion {
            found: registry.schema_version,
        });
    }
    if registry.workspaces.is_empty() {
        return Err(RegistryError::EmptyRegistry);
    }
    if !registry
        .workspaces
        .contains_key(&registry.default_workspace)
    {
        return Err(RegistryError::MissingDefault {
            default_workspace: registry.default_workspace.clone(),
        });
    }

    validate_selectors(registry)?;
    validate_ids(registry)?;
    validate_roots(registry)
}

fn validate_selectors(registry: &MachineRegistry) -> Result<(), RegistryError> {
    let mut owners = BTreeMap::<String, WorkspaceName>::new();
    for canonical_name in registry.workspaces.keys() {
        insert_selector(&mut owners, canonical_name.as_str(), canonical_name)?;
    }
    for (canonical_name, record) in &registry.workspaces {
        for alias in &record.aliases {
            insert_selector(&mut owners, alias.as_str(), canonical_name)?;
        }
    }
    Ok(())
}

fn insert_selector(
    owners: &mut BTreeMap<String, WorkspaceName>,
    selector: &str,
    workspace: &WorkspaceName,
) -> Result<(), RegistryError> {
    let selector = selector.to_ascii_lowercase();
    if let Some(first_workspace) = owners.insert(selector.clone(), workspace.clone()) {
        return Err(RegistryError::DuplicateSelector {
            selector,
            first_workspace,
            second_workspace: workspace.clone(),
        });
    }
    Ok(())
}

fn validate_ids(registry: &MachineRegistry) -> Result<(), RegistryError> {
    let mut ids = HashSet::with_capacity(registry.workspaces.len());
    for record in registry.workspaces.values() {
        if !ids.insert(record.workspace_id) {
            return Err(RegistryError::DuplicateWorkspaceId {
                workspace_id: record.workspace_id,
            });
        }
    }
    Ok(())
}

fn validate_roots(registry: &MachineRegistry) -> Result<(), RegistryError> {
    let mut roots: Vec<PathBuf> = Vec::with_capacity(registry.workspaces.len());
    for (canonical_name, record) in &registry.workspaces {
        if !record.root.is_absolute() {
            return Err(RegistryError::RelativeRoot {
                canonical_name: canonical_name.clone(),
                root: record.root.clone(),
            });
        }
        let normalized = normalize_root(&record.root, Path::new("/"))
            .expect("an absolute root never needs the supplied base");
        for previous in &roots {
            if paths_overlap(previous, &normalized) {
                return Err(RegistryError::OverlappingRoots {
                    first: previous.clone(),
                    second: normalized,
                });
            }
        }
        roots.push(normalized);
    }
    Ok(())
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    first == second || first.starts_with(second) || second.starts_with(first)
}
