//! Bounded interprocess serialization for workspace-registry transactions.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, ErrorCode};

use super::store::io_error;
use super::{RegistryError, RegistryOperation};

pub(super) struct Guard {
    _connection: Connection,
}

pub(super) fn acquire(path: &Path, timeout: Duration) -> Result<Guard, RegistryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            io_error(
                RegistryOperation::CreateDirectory,
                parent,
                Some(path),
                &error,
            )
        })?;
    }
    let connection = Connection::open(path).map_err(|error| lock_error(path, &error))?;
    connection
        .busy_timeout(timeout)
        .map_err(|error| lock_error(path, &error))?;
    match connection.execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE") {
        Ok(()) => {}
        Err(error) if is_contention(&error) => {
            return Err(RegistryError::LockTimeout {
                path: path.to_path_buf(),
                owner_pid: owner_pid(path),
                waited_millis: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            });
        }
        Err(error) => return Err(lock_error(path, &error)),
    }
    write_owner(path)?;
    Ok(Guard {
        _connection: connection,
    })
}

fn is_contention(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(failure.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

fn write_owner(lock_path: &Path) -> Result<(), RegistryError> {
    let path = owner_path(lock_path);
    std::fs::write(&path, format!("{}\n", std::process::id())).map_err(|error| {
        io_error(
            RegistryOperation::WriteTransactionLock,
            &path,
            Some(lock_path),
            &error,
        )
    })
}

fn owner_pid(lock_path: &Path) -> Option<u32> {
    std::fs::read_to_string(owner_path(lock_path))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn owner_path(lock_path: &Path) -> PathBuf {
    let mut path = lock_path.as_os_str().to_owned();
    path.push(".owner");
    PathBuf::from(path)
}

fn lock_error(path: &Path, error: &rusqlite::Error) -> RegistryError {
    RegistryError::Io {
        operation: RegistryOperation::AcquireTransactionLock,
        path: path.to_path_buf(),
        related_path: None,
        kind: std::io::ErrorKind::Other,
        message: error.to_string(),
    }
}
