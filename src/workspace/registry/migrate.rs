//! One-time conversion of the legacy flat machine environment into schema v2.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryError, RegistryOperation, RegistryStore,
    WorkspaceRecord,
};
use crate::workspace::{WorkspaceId, WorkspaceName, context::normalize_root};

/// The result of checking or creating the machine workspace registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// The validated schema-v2 registry available after the migration check.
    pub registry: MachineRegistry,
    /// Whether this call created and persisted a schema-v2 registry.
    pub created_registry: bool,
    /// Exact-byte legacy backup written before replacing a flat env file.
    pub backup_path: Option<PathBuf>,
    /// Whether portable workspace setup still needs to establish access data.
    pub portable_setup_required: bool,
}

/// Convert legacy flat environment bytes into one default workspace.
///
/// `config_dir` is the fixed machine config directory containing `env.json`.
/// Its parent may contain the read-only legacy `brain-root` pointer.
///
/// # Errors
///
/// Returns a typed registry storage or validation error if the schema-v2
/// registry cannot be persisted.
pub fn migrate_legacy(
    home: &Path,
    config_dir: &Path,
    legacy_body: &[u8],
) -> Result<MigrationOutcome, RegistryError> {
    migrate_legacy_with(home, config_dir, legacy_body, &Map::new())
}

pub(crate) fn migrate_legacy_with(
    home: &Path,
    config_dir: &Path,
    legacy_body: &[u8],
    fallback_env: &Map<String, Value>,
) -> Result<MigrationOutcome, RegistryError> {
    migrate_legacy_with_before_save(home, config_dir, legacy_body, fallback_env, || Ok(()))
}

fn migrate_legacy_with_before_save(
    home: &Path,
    config_dir: &Path,
    legacy_body: &[u8],
    fallback_env: &Map<String, Value>,
    before_save: impl FnOnce() -> Result<(), RegistryError>,
) -> Result<MigrationOutcome, RegistryError> {
    let env_path = config_dir.join("env.json");
    RegistryStore::from_path(env_path).transaction(|transaction| {
        let authoritative_body = match transaction.read_bytes() {
            Ok(body) => body,
            Err(RegistryError::Io {
                operation: RegistryOperation::ReadRegistry,
                kind: std::io::ErrorKind::NotFound,
                ..
            }) => legacy_body.to_vec(),
            Err(error) => return Err(error),
        };
        migrate_locked(
            transaction,
            home,
            config_dir,
            &authoritative_body,
            fallback_env,
            before_save,
        )
    })
}

fn migrate_locked(
    transaction: &super::store::RegistryTransaction<'_>,
    home: &Path,
    config_dir: &Path,
    legacy_body: &[u8],
    fallback_env: &Map<String, Value>,
    before_save: impl FnOnce() -> Result<(), RegistryError>,
) -> Result<MigrationOutcome, RegistryError> {
    if let Ok(registry) = serde_json::from_slice::<MachineRegistry>(legacy_body) {
        return Ok(MigrationOutcome {
            registry,
            created_registry: false,
            backup_path: None,
            portable_setup_required: false,
        });
    }

    let mut legacy = parse_flat_env(legacy_body);
    for (name, value) in fallback_env {
        legacy.entry(name.clone()).or_insert_with(|| value.clone());
    }
    let root = resolved_root(home, config_dir, &legacy);
    let canonical_name = WorkspaceName::from_root(&root)
        .unwrap_or_else(|_| WorkspaceName::parse("brain").expect("brain is a valid name"));
    let receiver_enabled = legacy
        .remove("receiver_enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    legacy.remove("root");
    legacy.remove("access_mode");
    legacy.remove("access_policy");

    let env_path = config_dir.join("env.json");
    let backup_path = env_path
        .is_file()
        .then(|| backup_legacy(&env_path, legacy_body))
        .transpose()?;
    fs::create_dir_all(&root).map_err(|error| RegistryError::Io {
        operation: RegistryOperation::CreateDirectory,
        path: root.clone(),
        related_path: Some(env_path.clone()),
        kind: error.kind(),
        message: error.to_string(),
    })?;
    let workspace_id =
        match crate::workspace::WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION")) {
            Ok(manifest) => manifest.workspace_id(),
            Err(crate::workspace::ManifestError::Io {
                kind: std::io::ErrorKind::NotFound,
                ..
            }) => {
                let workspace_id = WorkspaceId::new();
                crate::workspace::WorkspaceManifest::new(workspace_id)
                    .write_new(&root)
                    .map_err(RegistryError::Manifest)?;
                workspace_id
            }
            Err(error) => return Err(RegistryError::Manifest(error)),
        };
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled,
                env: legacy,
            },
        )]),
    };
    before_save()?;
    transaction.save(&registry)?;
    Ok(MigrationOutcome {
        registry,
        created_registry: true,
        backup_path,
        portable_setup_required: true,
    })
}

fn backup_legacy(env_path: &Path, body: &[u8]) -> Result<PathBuf, RegistryError> {
    for suffix in 0_u64.. {
        let backup_path = legacy_backup_path(env_path, suffix);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut backup = match options.open(&backup_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(migration_io_error(
                    RegistryOperation::CreateLegacyBackup,
                    &backup_path,
                    env_path,
                    &error,
                ));
            }
        };
        if let Err(error) = backup.write_all(body) {
            let _ = fs::remove_file(&backup_path);
            return Err(migration_io_error(
                RegistryOperation::WriteLegacyBackup,
                &backup_path,
                env_path,
                &error,
            ));
        }
        if let Err(error) = backup.sync_all() {
            let _ = fs::remove_file(&backup_path);
            return Err(migration_io_error(
                RegistryOperation::SyncLegacyBackup,
                &backup_path,
                env_path,
                &error,
            ));
        }
        return Ok(backup_path);
    }
    unreachable!("u64 backup suffixes cannot be exhausted")
}

fn legacy_backup_path(env_path: &Path, suffix: u64) -> PathBuf {
    let base = format!(
        "{}.legacy-backup",
        env_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("env.json")
    );
    let name = if suffix == 0 {
        base
    } else {
        format!("{base}.{suffix}")
    };
    env_path.with_file_name(name)
}

fn migration_io_error(
    operation: RegistryOperation,
    path: &Path,
    related_path: &Path,
    error: &std::io::Error,
) -> RegistryError {
    RegistryError::Io {
        operation,
        path: path.to_path_buf(),
        related_path: Some(related_path.to_path_buf()),
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn parse_flat_env(body: &[u8]) -> Map<String, Value> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn resolved_root(home: &Path, config_dir: &Path, legacy: &Map<String, Value>) -> PathBuf {
    let flat_root = legacy
        .get("root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|root| !root.is_empty());
    let pointer_root = config_dir
        .parent()
        .map(|parent| parent.join("brain-root"))
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|body| crate::paths::parse_brain_root_file(&body));
    let raw_root = flat_root.or(pointer_root.as_deref());
    let expanded = raw_root.map_or_else(
        || home.join("brain"),
        |root| crate::paths::expand_tilde_with_home(root, home),
    );
    normalize_root(&expanded, home).unwrap_or_else(|_| home.join("brain"))
}

#[cfg(test)]
mod tests {
    use super::{migrate_legacy_with, migrate_legacy_with_before_save};
    use crate::workspace::{RegistryError, RegistryOperation, WorkspaceManifest};
    use serde_json::Map;

    #[test]
    fn retry_after_post_manifest_save_failure_adopts_the_same_portable_identity() {
        let home = tempfile::tempdir().unwrap();
        let config_home = tempfile::tempdir().unwrap();
        let config_dir = config_home.path().join("brain");
        std::fs::create_dir_all(&config_dir).unwrap();
        let env_path = config_dir.join("env.json");
        let legacy = br#"{"root":"~/brain","custom":"keep"}"#;
        std::fs::write(&env_path, legacy).unwrap();

        let error =
            migrate_legacy_with_before_save(home.path(), &config_dir, legacy, &Map::new(), || {
                Err(RegistryError::Io {
                    operation: RegistryOperation::ReplaceRegistry,
                    path: env_path.clone(),
                    related_path: None,
                    kind: std::io::ErrorKind::Other,
                    message: "injected registry save failure".to_owned(),
                })
            })
            .unwrap_err();
        assert!(error.to_string().contains("injected registry save failure"));
        assert_eq!(std::fs::read(&env_path).unwrap(), legacy);
        let root = home.path().join("brain");
        let first_manifest_bytes = std::fs::read(WorkspaceManifest::path(&root)).unwrap();
        let first_manifest = WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION")).unwrap();

        let retried = migrate_legacy_with(home.path(), &config_dir, legacy, &Map::new()).unwrap();

        assert_eq!(
            retried.registry.select(None).unwrap().record().workspace_id,
            first_manifest.workspace_id()
        );
        assert_eq!(
            WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION"))
                .unwrap()
                .receiver_ingress_id(),
            first_manifest.receiver_ingress_id()
        );
        assert_eq!(
            std::fs::read(WorkspaceManifest::path(&root)).unwrap(),
            first_manifest_bytes
        );
    }
}
