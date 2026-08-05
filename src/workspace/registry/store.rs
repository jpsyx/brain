//! Registry path resolution, loading, atomic persistence, and transactions.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::lock;
use super::model::RawMachineRegistry;
use super::{MachineRegistry, ReceiverAction, RegistryError, RegistryOperation, validate_registry};
use crate::workspace::{WorkspaceId, WorkspaceName};

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A registry persistence boundary bound to one explicit path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryStore {
    path: PathBuf,
    temporary_path: Option<PathBuf>,
    lock_timeout: Duration,
}

const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

impl RegistryStore {
    /// Resolve the sole machine-global registry path.
    #[must_use]
    pub fn real() -> Self {
        Self::from_path(crate::paths::machine_config_dir().join("env.json"))
    }

    /// Bind a store to an injected path.
    #[must_use]
    pub fn from_path(path: PathBuf) -> Self {
        Self {
            path,
            temporary_path: None,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(super) fn from_path_with_temporary(path: PathBuf, temporary_path: PathBuf) -> Self {
        Self {
            path,
            temporary_path: Some(temporary_path),
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(super) fn from_path_with_lock_timeout(path: PathBuf, lock_timeout: Duration) -> Self {
        Self {
            path,
            temporary_path: None,
            lock_timeout,
        }
    }

    /// The registry file this store owns.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serialize a complete read-mutate-persist operation across processes.
    pub(crate) fn transaction<T, E>(
        &self,
        operation: impl FnOnce(&RegistryTransaction<'_>) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<RegistryError>,
    {
        let guard = lock::acquire(&self.lock_path(), self.lock_timeout).map_err(E::from)?;
        operation(&RegistryTransaction {
            store: self,
            _guard: guard,
        })
    }

    fn lock_path(&self) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("env.json");
        self.path
            .with_file_name(format!(".{file_name}.transaction.lock"))
    }

    /// Load and validate a registry from an explicit path.
    pub fn load_from(path: &Path) -> Result<MachineRegistry, RegistryError> {
        let bytes = fs::read(path)
            .map_err(|error| io_error(RegistryOperation::ReadRegistry, path, None, &error))?;
        let raw: RawMachineRegistry = serde_json::from_slice(&bytes)
            .map_err(|error| json_error(RegistryOperation::ParseRegistry, path, &error))?;
        MachineRegistry::try_from(raw)
    }

    /// Replace this store's registry inside an interprocess transaction.
    pub fn replace(&self, registry: &MachineRegistry) -> Result<(), RegistryError> {
        self.transaction(|transaction| transaction.save(registry))
    }

    /// Validate and atomically save a registry to an explicit path.
    pub(crate) fn save_atomic_to(
        path: &Path,
        registry: &MachineRegistry,
    ) -> Result<(), RegistryError> {
        let temporary = unique_temporary_path(path);
        Self::save_atomic_to_with_temporary(path, registry, &temporary)
    }

    fn save_atomic_to_with_temporary(
        path: &Path,
        registry: &MachineRegistry,
        temporary: &Path,
    ) -> Result<(), RegistryError> {
        validate_registry(registry)?;
        let mut bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| json_error(RegistryOperation::SerializeRegistry, path, &error))?;
        bytes.push(b'\n');

        let parent = parent_or_current_dir(path);
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                RegistryOperation::CreateDirectory,
                parent,
                Some(path),
                &error,
            )
        })?;
        let result = write_and_replace(temporary, path, &bytes);
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    /// Run one persisted transaction across the registry file and live value.
    ///
    /// This acquires the interprocess lock before reloading current state,
    /// clones and mutates a candidate, validates and atomically persists it,
    /// and only then replaces `registry`. The mutation methods directly on
    /// [`MachineRegistry`] provide the same rollback behavior in memory but do
    /// not write a registry file.
    pub fn update<T>(
        &self,
        registry: &mut MachineRegistry,
        mutation: impl FnOnce(&mut MachineRegistry) -> Result<T, RegistryError>,
    ) -> Result<T, RegistryError> {
        self.transaction(|transaction| {
            let mut latest = transaction.load()?;
            let result = transaction.update(&mut latest, mutation)?;
            *registry = latest;
            Ok(result)
        })
    }

    /// Reload and mutate receiver intent for the exact selected record.
    pub fn transition_receiver(
        &self,
        canonical_name: &WorkspaceName,
        expected_id: WorkspaceId,
        action: ReceiverAction,
    ) -> Result<bool, RegistryError> {
        self.transaction(|transaction| {
            let mut latest = transaction.load()?;
            transaction.update(&mut latest, |registry| {
                registry.transition_receiver(canonical_name, expected_id, action)
            })
        })
    }
}

/// A held registry transaction. Loading and persistence remain unavailable
/// until `RegistryStore` has acquired the interprocess lock.
pub(crate) struct RegistryTransaction<'a> {
    store: &'a RegistryStore,
    _guard: lock::Guard,
}

impl RegistryTransaction<'_> {
    pub(crate) fn load(&self) -> Result<MachineRegistry, RegistryError> {
        RegistryStore::load_from(&self.store.path)
    }

    pub(crate) fn read_bytes(&self) -> Result<Vec<u8>, RegistryError> {
        fs::read(&self.store.path).map_err(|error| {
            io_error(
                RegistryOperation::ReadRegistry,
                &self.store.path,
                None,
                &error,
            )
        })
    }

    pub(crate) fn save(&self, registry: &MachineRegistry) -> Result<(), RegistryError> {
        self.store.temporary_path.as_ref().map_or_else(
            || RegistryStore::save_atomic_to(&self.store.path, registry),
            |temporary| {
                RegistryStore::save_atomic_to_with_temporary(&self.store.path, registry, temporary)
            },
        )
    }

    pub(crate) fn update<T>(
        &self,
        registry: &mut MachineRegistry,
        mutation: impl FnOnce(&mut MachineRegistry) -> Result<T, RegistryError>,
    ) -> Result<T, RegistryError> {
        let mut candidate = registry.clone();
        let result = mutation(&mut candidate)?;
        validate_registry(&candidate)?;
        self.save(&candidate)?;
        *registry = candidate;
        Ok(result)
    }
}

pub(super) fn unique_temporary_path(path: &Path) -> PathBuf {
    let parent = parent_or_current_dir(path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("registry");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let counter = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{file_name}.tmp-{}-{nonce}-{counter}",
        std::process::id()
    ))
}

pub(super) fn parent_or_current_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn write_and_replace(
    temporary: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), RegistryError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temporary).map_err(|error| {
        io_error(
            RegistryOperation::CreateTemporary,
            temporary,
            Some(destination),
            &error,
        )
    })?;
    file.write_all(bytes).map_err(|error| {
        io_error(
            RegistryOperation::WriteTemporary,
            temporary,
            Some(destination),
            &error,
        )
    })?;
    file.sync_all().map_err(|error| {
        io_error(
            RegistryOperation::SyncTemporary,
            temporary,
            Some(destination),
            &error,
        )
    })?;
    drop(file);
    fs::rename(temporary, destination).map_err(|error| {
        io_error(
            RegistryOperation::ReplaceRegistry,
            destination,
            Some(temporary),
            &error,
        )
    })?;

    let parent = parent_or_current_dir(destination);
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

pub(super) fn io_error(
    operation: RegistryOperation,
    path: &Path,
    related_path: Option<&Path>,
    error: &std::io::Error,
) -> RegistryError {
    RegistryError::Io {
        operation,
        path: path.to_path_buf(),
        related_path: related_path.map(Path::to_path_buf),
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn json_error(
    operation: RegistryOperation,
    path: &Path,
    error: &serde_json::Error,
) -> RegistryError {
    RegistryError::Json {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
