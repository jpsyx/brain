//! Serializable registry records and validated mutation operations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Formatter;
use std::path::PathBuf;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{RegistryError, validate_registry};
use crate::workspace::{WorkspaceId, WorkspaceName};

/// The only registry schema this release accepts.
pub const REGISTRY_SCHEMA_VERSION: u32 = 2;

/// Every workspace attached to this machine and the canonical default.
///
/// Successful deserialization establishes all whole-registry invariants.
/// Because the fields remain public data, direct field mutation must be
/// followed by [`validate_registry`] or performed through a validated method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachineRegistry {
    /// Persisted schema discriminator.
    pub schema_version: u32,
    /// Canonical name selected when no selector is supplied.
    pub default_workspace: WorkspaceName,
    /// Siloed records keyed by canonical workspace name.
    #[serde(deserialize_with = "deserialize_workspaces")]
    pub workspaces: BTreeMap<WorkspaceName, WorkspaceRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawMachineRegistry {
    schema_version: u32,
    default_workspace: WorkspaceName,
    #[serde(deserialize_with = "deserialize_workspaces")]
    workspaces: BTreeMap<WorkspaceName, WorkspaceRecord>,
}

/// Machine-local configuration and access data for one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecord {
    /// Stable identity that survives renames.
    pub workspace_id: WorkspaceId,
    /// Workspace content root on this machine.
    pub root: PathBuf,
    /// Alternative selectors for this workspace.
    #[serde(default, deserialize_with = "deserialize_aliases")]
    pub aliases: BTreeSet<WorkspaceName>,
    /// User identity used within this workspace on this machine.
    pub local_user_id: String,
    /// Whether this workspace accepts receiver traffic.
    #[serde(default)]
    pub receiver_enabled: bool,
    /// Workspace-siloed machine environment and access data.
    #[serde(default)]
    pub env: Map<String, Value>,
}

fn deserialize_workspaces<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<WorkspaceName, WorkspaceRecord>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct WorkspaceMapVisitor;

    impl<'de> Visitor<'de> for WorkspaceMapVisitor {
        type Value = BTreeMap<WorkspaceName, WorkspaceRecord>;

        fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map of unique canonical workspace names")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut workspaces = BTreeMap::new();
            while let Some((raw_name, record)) = map.next_entry::<String, WorkspaceRecord>()? {
                let canonical_name =
                    WorkspaceName::parse(&raw_name).map_err(serde::de::Error::custom)?;
                if workspaces.insert(canonical_name, record).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate canonical workspace selector {raw_name}"
                    )));
                }
            }
            Ok(workspaces)
        }
    }

    deserializer.deserialize_map(WorkspaceMapVisitor)
}

fn deserialize_aliases<'de, D>(deserializer: D) -> Result<BTreeSet<WorkspaceName>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw_aliases = Vec::<String>::deserialize(deserializer)?;
    let mut aliases = BTreeSet::new();
    for raw_alias in raw_aliases {
        let alias = WorkspaceName::parse(&raw_alias).map_err(serde::de::Error::custom)?;
        if !aliases.insert(alias) {
            return Err(serde::de::Error::custom(format!(
                "duplicate canonical workspace alias {raw_alias}"
            )));
        }
    }
    Ok(aliases)
}

impl TryFrom<RawMachineRegistry> for MachineRegistry {
    type Error = RegistryError;

    fn try_from(raw: RawMachineRegistry) -> Result<Self, Self::Error> {
        let registry = Self {
            schema_version: raw.schema_version,
            default_workspace: raw.default_workspace,
            workspaces: raw.workspaces,
        };
        validate_registry(&registry)?;
        Ok(registry)
    }
}

impl<'de> Deserialize<'de> for MachineRegistry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::try_from(RawMachineRegistry::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

impl MachineRegistry {
    // These methods are in-memory transactions. RegistryStore::update adds the
    // persisted transaction boundary when both memory and disk must advance.

    /// Create and attach a fresh workspace record.
    pub fn create_record(
        &mut self,
        canonical_name: WorkspaceName,
        root: PathBuf,
        local_user_id: impl Into<String>,
    ) -> Result<WorkspaceId, RegistryError> {
        let workspace_id = WorkspaceId::new();
        let record = WorkspaceRecord {
            workspace_id,
            root,
            aliases: BTreeSet::new(),
            local_user_id: local_user_id.into(),
            receiver_enabled: false,
            env: Map::new(),
        };
        self.attach_record(canonical_name, record)?;
        Ok(workspace_id)
    }

    /// Attach an existing record without changing its stable identity.
    pub fn attach_record(
        &mut self,
        canonical_name: WorkspaceName,
        record: WorkspaceRecord,
    ) -> Result<(), RegistryError> {
        self.mutate(|candidate| {
            if candidate.workspaces.contains_key(&canonical_name) {
                return Err(RegistryError::WorkspaceAlreadyExists {
                    canonical_name: canonical_name.clone(),
                });
            }
            if candidate.workspaces.is_empty() {
                candidate.default_workspace.clone_from(&canonical_name);
            }
            candidate.workspaces.insert(canonical_name, record);
            Ok(())
        })
    }

    /// Atomically rekey one canonical name while preserving its record.
    pub fn rename(
        &mut self,
        current_name: &str,
        new_name: WorkspaceName,
    ) -> Result<(), RegistryError> {
        self.mutate(|candidate| {
            let current = WorkspaceName::parse(current_name).map_err(|_| {
                RegistryError::UnknownWorkspace {
                    canonical_name: current_name.to_owned(),
                }
            })?;
            if candidate.workspaces.contains_key(&new_name) {
                return Err(RegistryError::WorkspaceAlreadyExists {
                    canonical_name: new_name.clone(),
                });
            }
            let record = candidate.workspaces.remove(&current).ok_or_else(|| {
                RegistryError::UnknownWorkspace {
                    canonical_name: current_name.to_owned(),
                }
            })?;
            if candidate.default_workspace == current {
                candidate.default_workspace.clone_from(&new_name);
            }
            candidate.workspaces.insert(new_name, record);
            Ok(())
        })
    }

    /// Add a validated alias to a canonical record.
    pub fn add_alias(
        &mut self,
        canonical_name: &str,
        alias: WorkspaceName,
    ) -> Result<(), RegistryError> {
        self.mutate(|candidate| {
            candidate.record_mut(canonical_name)?.aliases.insert(alias);
            Ok(())
        })
    }

    /// Remove an alias from a canonical record.
    pub fn remove_alias(&mut self, canonical_name: &str, alias: &str) -> Result<(), RegistryError> {
        self.mutate(|candidate| {
            let alias = WorkspaceName::parse(alias).map_err(|_| RegistryError::UnknownAlias {
                alias: alias.to_owned(),
            })?;
            if !candidate.record_mut(canonical_name)?.aliases.remove(&alias) {
                return Err(RegistryError::UnknownAlias {
                    alias: alias.to_string(),
                });
            }
            Ok(())
        })
    }

    /// Change the default to the canonical record named by a selector.
    pub fn set_default(&mut self, selector: &str) -> Result<(), RegistryError> {
        let canonical_name = self.select(Some(selector))?.canonical_name().clone();
        self.mutate(|candidate| {
            candidate.default_workspace = canonical_name;
            Ok(())
        })
    }

    /// Detach a non-default record without touching its root.
    pub fn remove(&mut self, selector: &str) -> Result<WorkspaceRecord, RegistryError> {
        let canonical_name = self.select(Some(selector))?.canonical_name().clone();
        self.mutate(|candidate| {
            candidate.workspaces.remove(&canonical_name).ok_or_else(|| {
                RegistryError::UnknownWorkspace {
                    canonical_name: canonical_name.to_string(),
                }
            })
        })
    }

    fn record_mut(&mut self, canonical_name: &str) -> Result<&mut WorkspaceRecord, RegistryError> {
        let name =
            WorkspaceName::parse(canonical_name).map_err(|_| RegistryError::UnknownWorkspace {
                canonical_name: canonical_name.to_owned(),
            })?;
        self.workspaces
            .get_mut(&name)
            .ok_or_else(|| RegistryError::UnknownWorkspace {
                canonical_name: canonical_name.to_owned(),
            })
    }

    fn mutate<T>(
        &mut self,
        mutation: impl FnOnce(&mut Self) -> Result<T, RegistryError>,
    ) -> Result<T, RegistryError> {
        let mut candidate = self.clone();
        let result = mutation(&mut candidate)?;
        validate_registry(&candidate)?;
        *self = candidate;
        Ok(result)
    }
}
