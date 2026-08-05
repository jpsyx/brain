//! Failure-atomic persistence for one selected receiver setup.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

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
    files: Vec<FileSnapshot>,
    directories: Vec<DirectorySnapshot>,
}

struct FileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    #[cfg(unix)]
    mode: Option<u32>,
}

struct DirectorySnapshot {
    path: PathBuf,
    existed: bool,
}

impl SetupSnapshot {
    fn capture(context: &CommandContext, home: &Path) -> Result<Self> {
        let root = context.workspace.root();
        let paths = [
            crate::users::UsersStore::path(&context.workspace),
            root.join(".claude/brain-hooks/claude_session_start_hook.py"),
            root.join(".claude/brain-hooks/claude_stop_hook.py"),
            root.join(".claude/settings.json"),
            root.join(".claude/.settings.json.transaction.lock"),
            home.join(".codex/hooks.json"),
            home.join(".codex/.hooks.json.transaction.lock"),
        ];
        let files = paths
            .into_iter()
            .map(FileSnapshot::capture)
            .collect::<Result<Vec<_>>>()?;
        let directories = [
            root.join(".claude/brain-hooks"),
            root.join(".claude"),
            home.join(".codex"),
        ]
        .into_iter()
        .map(|path| DirectorySnapshot {
            existed: path.is_dir(),
            path,
        })
        .collect();
        Ok(Self {
            providers: crate::env::load_map(context),
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
        if let Err(error) = crate::env::replace_map(context, &self.providers) {
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
            #[cfg(unix)]
            mode,
        })
    }

    fn restore(&self) -> Result<()> {
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
    let snapshot = SetupSnapshot::capture(context, home)?;
    let result = (|| {
        crate::env::set_many(context, &plan.providers)?;
        after_write(CommitStep::Providers)?;
        crate::users::UsersStore::save(&context.workspace, &plan.users)?;
        after_write(CommitStep::Users)?;
        hooks::install_for_home_with(context.workspace.root(), home, |step| {
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
