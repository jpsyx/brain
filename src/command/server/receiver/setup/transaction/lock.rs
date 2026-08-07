use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use fs2::FileExt as _;

const SETUP_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const SETUP_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub(super) enum SetupLockError {
    Timeout {
        path: PathBuf,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for SetupLockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { path } => write!(
                formatter,
                "receiver setup lock timed out at {}; another setup may be suspended",
                path.display()
            ),
            Self::Io { path, source } => write!(
                formatter,
                "receiver setup lock failed at {}: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SetupLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

#[derive(Debug)]
pub(super) struct SetupTransactionLock {
    _file: std::fs::File,
}

impl SetupTransactionLock {
    pub(super) fn acquire(root: &Path) -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(SETUP_LOCK_TIMEOUT)
            .context("receiver setup lock deadline exceeds the monotonic clock range")?;
        Self::acquire_until(root, deadline, &Instant::now, &std::thread::park_timeout)
            .map_err(Into::into)
    }

    pub(super) fn acquire_until(
        root: &Path,
        deadline: Instant,
        clock: &impl Fn() -> Instant,
        poll: &impl Fn(Duration),
    ) -> std::result::Result<Self, SetupLockError> {
        let path = root.join(".config/.receiver-setup.transaction.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SetupLockError::Io {
                path: path.clone(),
                source,
            })?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|source| SetupLockError::Io {
            path: path.clone(),
            source,
        })?;
        loop {
            if clock() >= deadline {
                return Err(SetupLockError::Timeout { path });
            }
            match file.try_lock_exclusive() {
                Ok(()) => {
                    if clock() >= deadline {
                        return Err(SetupLockError::Timeout { path });
                    }
                    return Ok(Self { _file: file });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    let now = clock();
                    if now >= deadline {
                        return Err(SetupLockError::Timeout { path });
                    }
                    poll(SETUP_LOCK_POLL_INTERVAL.min(deadline.duration_since(now)));
                }
                Err(source) => {
                    return Err(SetupLockError::Io { path, source });
                }
            }
        }
    }
}
