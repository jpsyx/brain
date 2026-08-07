//! Failure-atomic persistence for one selected receiver setup.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use fs2::FileExt as _;

use super::SetupPlan;
use crate::command::server::receiver::hooks::{self, InstallStep};
use crate::workspace::CommandContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitStep {
    Providers,
    Users,
    Hook(InstallStep),
}

struct SetupSnapshot {
    providers: serde_json::Map<String, serde_json::Value>,
    provider_writes: Vec<(&'static str, String)>,
    files: Vec<FileSnapshot>,
    directories: Vec<DirectorySnapshot>,
}

struct FileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    written_bytes: Option<Vec<u8>>,
    #[cfg(unix)]
    mode: Option<u32>,
}

struct DirectorySnapshot {
    path: PathBuf,
    existed: bool,
}

const SETUP_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const SETUP_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
enum SetupLockError {
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
struct SetupTransactionLock {
    _file: std::fs::File,
}

impl SetupTransactionLock {
    fn acquire(root: &Path) -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(SETUP_LOCK_TIMEOUT)
            .context("receiver setup lock deadline exceeds the monotonic clock range")?;
        Self::acquire_until(root, deadline, &Instant::now, &std::thread::park_timeout)
            .map_err(Into::into)
    }

    fn acquire_until(
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

impl SetupSnapshot {
    fn capture(context: &CommandContext, home: &Path) -> Result<Self> {
        let root = context.workspace.root();
        let paths = [
            crate::users::UsersStore::path(&context.workspace),
            root.join(".claude/brain-hooks/claude_session_start_hook.py"),
            root.join(".claude/brain-hooks/claude_stop_hook.py"),
            root.join(".claude/settings.json"),
            home.join(".codex/hooks.json"),
            root.join(".opencode/plugins/brain.js"),
        ];
        let files = paths
            .into_iter()
            .map(FileSnapshot::capture)
            .collect::<Result<Vec<_>>>()?;
        let directories = [
            root.join(".claude/brain-hooks"),
            root.join(".claude"),
            home.join(".codex"),
            root.join(".opencode/plugins"),
            root.join(".opencode"),
        ]
        .into_iter()
        .map(|path| DirectorySnapshot {
            existed: path.is_dir(),
            path,
        })
        .collect();
        Ok(Self {
            providers: crate::env::load_map(context),
            provider_writes: Vec::new(),
            files,
            directories,
        })
    }

    fn restore(&self, context: &CommandContext) -> Result<()> {
        let mut failures = Vec::new();
        for file in self.files.iter().rev() {
            if let Err(error) = file.restore() {
                failures.push(format!("restore {}: {error:#}", file.path.display()));
            }
        }
        if let Err(error) =
            crate::env::restore_values_if_unchanged(context, &self.providers, &self.provider_writes)
        {
            failures.push(format!("restore selected provider record: {error:#}"));
        }
        for directory in &self.directories {
            if directory.existed {
                continue;
            }
            match std::fs::remove_dir(&directory.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => failures.push(format!(
                    "remove rollback directory {}: {error}",
                    directory.path.display()
                )),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> Result<Self> {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("snapshot {}", path.display()));
            }
        };
        #[cfg(unix)]
        let mode = match std::fs::metadata(&path) {
            Ok(metadata) => {
                use std::os::unix::fs::PermissionsExt as _;
                Some(metadata.permissions().mode() & 0o777)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("snapshot permissions for {}", path.display()));
            }
        };
        Ok(Self {
            path,
            bytes,
            written_bytes: None,
            #[cfg(unix)]
            mode,
        })
    }

    fn restore(&self) -> Result<()> {
        let Some(written) = &self.written_bytes else {
            return Ok(());
        };
        match std::fs::read(&self.path) {
            Ok(current) if current != *written => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", self.path.display()));
            }
            Ok(_) => {}
        }
        let Some(bytes) = &self.bytes else {
            return match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error).with_context(|| format!("remove {}", self.path.display())),
            };
        };
        replace_file(
            &self.path,
            bytes,
            #[cfg(unix)]
            self.mode.unwrap_or(0o600),
        )
    }
}

impl SetupSnapshot {
    fn record_providers(&mut self, providers: &[(&'static str, String)]) {
        self.provider_writes = providers.to_vec();
    }

    fn record_file(&mut self, path: &Path) -> Result<()> {
        let Some(snapshot) = self.files.iter_mut().find(|file| file.path == path) else {
            return Ok(());
        };
        snapshot.written_bytes = Some(
            std::fs::read(path).with_context(|| format!("record write to {}", path.display()))?,
        );
        Ok(())
    }

    fn record_hook_step(&mut self, root: &Path, home: &Path, step: InstallStep) -> Result<()> {
        let path = match step {
            InstallStep::SessionScript => {
                root.join(".claude/brain-hooks/claude_session_start_hook.py")
            }
            InstallStep::StopScript => root.join(".claude/brain-hooks/claude_stop_hook.py"),
            InstallStep::ClaudeSettings => root.join(".claude/settings.json"),
            InstallStep::CodexSettings => home.join(".codex/hooks.json"),
            InstallStep::OpenCodePlugin => root.join(".opencode/plugins/brain.js"),
        };
        self.record_file(&path)
    }
}

pub(super) fn persist_plan(plan: &SetupPlan, context: &CommandContext) -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    persist_plan_with_hook(plan, context, &home, |_| Ok(()))
}

fn persist_plan_with_hook(
    plan: &SetupPlan,
    context: &CommandContext,
    home: &Path,
    mut after_write: impl FnMut(CommitStep) -> Result<()>,
) -> Result<()> {
    let _transaction = SetupTransactionLock::acquire(context.workspace.root())?;
    let mut snapshot = SetupSnapshot::capture(context, home)?;
    let result = (|| {
        crate::env::set_many(context, &plan.providers)?;
        snapshot.record_providers(&plan.providers);
        after_write(CommitStep::Providers)?;
        crate::users::UsersStore::save(&context.workspace, &plan.users)?;
        snapshot.record_file(&crate::users::UsersStore::path(&context.workspace))?;
        after_write(CommitStep::Users)?;
        hooks::install_for_home_with(context.workspace.root(), home, |step| {
            snapshot.record_hook_step(context.workspace.root(), home, step)?;
            after_write(CommitStep::Hook(step))
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        return match snapshot.restore(context) {
            Ok(()) => Err(error),
            Err(rollback) => {
                Err(error.context(format!("receiver setup rollback also failed: {rollback:#}")))
            }
        };
    }
    Ok(())
}

fn replace_file(path: &Path, bytes: &[u8], #[cfg(unix)] mode: u32) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create rollback directory {}", parent.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let temporary = path.with_file_name(format!(".{file_name}.rollback-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create rollback file {}", temporary.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .with_context(|| format!("write rollback file {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync rollback file {}", temporary.display()))?;
        drop(file);
        std::fs::rename(&temporary, path).with_context(|| {
            format!(
                "replace {} from rollback file {}",
                path.display(),
                temporary.display()
            )
        })?;
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests;
