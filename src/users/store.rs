//! Portable users path resolution and same-directory atomic persistence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Users, UsersError};
use crate::workspace::WorkspaceContext;

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The portable user registry store.
pub struct UsersStore;

impl UsersStore {
    /// Resolve `<workspace-root>/.config/users.json`.
    #[must_use]
    pub fn path(workspace: &WorkspaceContext) -> PathBuf {
        workspace.root().join(".config/users.json")
    }

    /// Load and validate the selected workspace's portable users.
    pub fn load(workspace: &WorkspaceContext) -> Result<Users, UsersError> {
        super::transaction::recover_pending(
            workspace.root(),
            &workspace.paths().user_transaction_lock(),
        )?;
        Self::load_from(&Self::path(workspace))
    }

    /// Load and validate portable users from an injected path.
    pub fn load_from(path: &Path) -> Result<Users, UsersError> {
        let bytes =
            fs::read(path).map_err(|error| io_error("read portable users", path, None, &error))?;
        Users::parse(&bytes)
    }

    /// Atomically replace the selected workspace's portable users.
    pub fn save(workspace: &WorkspaceContext, users: &Users) -> Result<(), UsersError> {
        Self::save_to(&Self::path(workspace), users)
    }

    /// Atomically replace an injected portable-users path.
    pub fn save_to(path: &Path, users: &Users) -> Result<(), UsersError> {
        let bytes = users.to_bytes()?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                "create portable users directory",
                parent,
                Some(path),
                &error,
            )
        })?;
        let temporary = temporary_path(path);
        let result = write_and_replace(&temporary, path, &bytes);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let counter = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("users.json");
    path.with_file_name(format!(
        ".{name}.tmp-{}-{nonce}-{counter}",
        std::process::id()
    ))
}

fn write_and_replace(temporary: &Path, path: &Path, bytes: &[u8]) -> Result<(), UsersError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temporary).map_err(|error| {
        io_error(
            "create temporary portable users",
            temporary,
            Some(path),
            &error,
        )
    })?;
    file.write_all(bytes).map_err(|error| {
        io_error(
            "write temporary portable users",
            temporary,
            Some(path),
            &error,
        )
    })?;
    file.sync_all().map_err(|error| {
        io_error(
            "sync temporary portable users",
            temporary,
            Some(path),
            &error,
        )
    })?;
    drop(file);
    fs::rename(temporary, path)
        .map_err(|error| io_error("replace portable users", path, Some(temporary), &error))?;
    if let Some(parent) = path.parent()
        && let Ok(directory) = fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn io_error(
    operation: &'static str,
    path: &Path,
    related_path: Option<&Path>,
    error: &std::io::Error,
) -> UsersError {
    UsersError::Io {
        operation,
        path: path.to_path_buf(),
        related_path: related_path.map(Path::to_path_buf),
        kind: error.kind(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod transaction_tests {
    use std::io;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::super::transaction::{
        FileChange, TransactionStep, journal_path, recover_pending, replace_group_with_hook,
    };

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Vec<FileChange>) {
        let root = tempfile::tempdir().unwrap();
        let lock = root.path().join("runtime/users.transaction.lock");
        let users = root.path().join(".config/users.json");
        let tasks = root.path().join("tasks/tasks.csv");
        std::fs::create_dir_all(users.parent().unwrap()).unwrap();
        std::fs::create_dir_all(tasks.parent().unwrap()).unwrap();
        std::fs::write(&users, b"old users").unwrap();
        std::fs::write(&tasks, b"old tasks").unwrap();
        let changes = vec![
            FileChange::new(tasks, b"old tasks".to_vec(), b"new tasks".to_vec()),
            FileChange::new(users, b"old users".to_vec(), b"new users".to_vec()),
        ];
        (root, lock, changes)
    }

    #[test]
    fn replacement_failure_rolls_every_file_back_to_the_old_generation() {
        let (root, lock, changes) = fixture();

        let error = replace_group_with_hook(root.path(), &lock, changes.clone(), |step| {
            if step == TransactionStep::Install(1) {
                return Err(io::Error::other("injected second replacement failure"));
            }
            Ok(())
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("injected second replacement failure")
        );
        assert_eq!(std::fs::read(&changes[0].path).unwrap(), b"old tasks");
        assert_eq!(std::fs::read(&changes[1].path).unwrap(), b"old users");
        assert!(!journal_path(root.path()).exists());
    }

    #[test]
    fn a_pending_journal_recovers_the_old_generation_after_interruption() {
        let (root, lock, changes) = fixture();

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _ = replace_group_with_hook(root.path(), &lock, changes.clone(), |step| {
                assert_ne!(step, TransactionStep::Install(1), "injected crash");
                Ok(())
            });
        }));
        assert!(interrupted.is_err());
        assert!(journal_path(root.path()).is_file());

        recover_pending(root.path(), &lock).unwrap();

        assert_eq!(std::fs::read(&changes[0].path).unwrap(), b"old tasks");
        assert_eq!(std::fs::read(&changes[1].path).unwrap(), b"old users");
        assert!(!journal_path(root.path()).exists());
    }

    #[test]
    fn rollback_failure_is_reported_and_remains_recoverable() {
        let (root, lock, changes) = fixture();

        let error =
            replace_group_with_hook(root.path(), &lock, changes.clone(), |step| match step {
                TransactionStep::Install(1) => {
                    Err(io::Error::other("injected replacement failure"))
                }
                TransactionStep::Restore(0) => Err(io::Error::other("injected rollback failure")),
                _ => Ok(()),
            })
            .unwrap_err();

        assert!(error.to_string().contains("rollback also failed"));
        assert!(error.to_string().contains("injected rollback failure"));
        assert!(journal_path(root.path()).is_file());
        recover_pending(root.path(), &lock).unwrap();
        assert_eq!(std::fs::read(&changes[0].path).unwrap(), b"old tasks");
        assert_eq!(std::fs::read(&changes[1].path).unwrap(), b"old users");
    }

    #[test]
    fn staging_failure_removes_every_previously_staged_file() {
        let (root, lock, changes) = fixture();

        replace_group_with_hook(root.path(), &lock, changes, |step| {
            if step == TransactionStep::Stage(1) {
                return Err(io::Error::other("injected second staging failure"));
            }
            Ok(())
        })
        .unwrap_err();

        for directory in [root.path().join(".config"), root.path().join("tasks")] {
            let names = std::fs::read_dir(directory)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(!names.iter().any(|name| name.contains(".brain-user-")));
        }
    }

    #[test]
    fn recovery_removes_pre_journal_artifacts_left_by_an_interruption() {
        let (root, lock, changes) = fixture();

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _ = replace_group_with_hook(root.path(), &lock, changes, |step| {
                assert_ne!(step, TransactionStep::Stage(1), "injected crash");
                Ok(())
            });
        }));
        assert!(interrupted.is_err());
        assert!(
            artifact_names(root.path())
                .iter()
                .any(|name| name.contains(".brain-user-"))
        );

        recover_pending(root.path(), &lock).unwrap();

        assert!(
            artifact_names(root.path())
                .iter()
                .all(|name| !name.contains(".brain-user-"))
        );
    }

    fn artifact_names(root: &std::path::Path) -> Vec<String> {
        [root.join(".config"), root.join("tasks")]
            .into_iter()
            .flat_map(|directory| {
                std::fs::read_dir(directory)
                    .unwrap()
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            })
            .collect()
    }
}
