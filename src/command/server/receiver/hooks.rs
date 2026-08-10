//! Workspace-sensitive lifecycle integration for configured agent frontends.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

fn replace_entry(
    settings: &mut serde_json::Value,
    event: &str,
    hook_basenames: &[&str],
    command: &str,
) {
    let hooks = settings
        .as_object_mut()
        .expect("settings JSON root is an object")
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let events = hooks
        .as_object_mut()
        .expect("hooks JSON is an object")
        .entry(event)
        .or_insert_with(|| serde_json::json!([]));
    let list = events.as_array_mut().expect("hook event is an array");
    list.retain_mut(|entry| {
        let Some(items) = entry
            .get_mut("hooks")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return true;
        };
        items.retain(|item| {
            !item
                .get("command")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| {
                    let candidate = candidate.trim_end_matches(['"', '\'']);
                    hook_basenames
                        .iter()
                        .any(|basename| candidate.ends_with(basename))
                })
        });
        !items.is_empty()
    });
    list.push(serde_json::json!({"hooks": [{"type": "command", "command": command}]}));
}

fn command(hook_path: &Path, root: &Path) -> String {
    hook_path.strip_prefix(root).map_or_else(
        |_| format!("python3 {}", hook_path.to_string_lossy()),
        |relative| format!("python3 {}", relative.to_string_lossy()),
    )
}

fn portable_root_command(hook_path: &Path) -> String {
    let relative = hook_path
        .to_string_lossy()
        .trim_start_matches('/')
        .to_owned();
    format!(r#"python3 "${{BRAIN_ROOT:-$HOME/brain}}/{relative}""#)
}

fn hook_lock_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("hooks.json");
    path.with_file_name(format!(".{file_name}.transaction.lock"))
}

fn hook_temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("hooks.json");
    path.with_file_name(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
fn update_json_file(path: &Path, mutation: impl FnOnce(&mut serde_json::Value)) -> Result<()> {
    update_json_file_with_temporary(path, &hook_temporary_path(path), mutation)
}

#[cfg(test)]
fn update_json_file_with_temporary(
    path: &Path,
    temporary: &Path,
    mutation: impl FnOnce(&mut serde_json::Value),
) -> Result<()> {
    update_json_file_with_temporary_and_lock(path, temporary, mutation, || Ok(()))
}

fn update_json_file_with_temporary_and_lock(
    path: &Path,
    temporary: &Path,
    mutation: impl FnOnce(&mut serde_json::Value),
    after_lock_created: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create hook settings directory {}", parent.display()))?;
    let lock_path = hook_lock_path(path);
    let lock_existed = lock_path.exists();
    let result = (|| {
        let connection = rusqlite::Connection::open(&lock_path)
            .with_context(|| format!("open hook settings lock {}", lock_path.display()))?;
        after_lock_created()?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .context("configure hook settings lock")?;
        connection
            .execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")
            .with_context(|| format!("acquire hook settings lock {}", lock_path.display()))?;

        let existing = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read hook settings {}", path.display()));
            }
        };
        let mut settings = match existing.as_deref() {
            Some(bytes) => serde_json::from_slice(bytes)
                .with_context(|| format!("parse hook settings {}", path.display()))?,
            None => serde_json::json!({}),
        };
        mutation(&mut settings);
        let mut bytes = serde_json::to_vec_pretty(&settings)
            .with_context(|| format!("serialize hook settings {}", path.display()))?;
        bytes.push(b'\n');
        // Reinstallation is idempotent on disk: settings that already say what
        // this Brain would write keep their mtime, so the workspace watcher has
        // nothing to push. See `needs_rewrite`.
        if existing.as_deref() == Some(bytes.as_slice()) {
            return Ok(());
        }

        write_and_replace_json(temporary, path, &bytes)
    })();
    if result.is_err() && !lock_existed {
        let _ = std::fs::remove_file(lock_path);
    }
    result
}

fn write_and_replace_json(temporary: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
    let write_destination = match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(destination)
            .with_context(|| format!("resolve hook settings target {}", destination.display()))?,
        Ok(_) => destination.to_path_buf(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => destination.to_path_buf(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect hook settings {}", destination.display()));
        }
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(temporary)
        .with_context(|| format!("create temporary hook settings {}", temporary.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .with_context(|| format!("write temporary hook settings {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary hook settings {}", temporary.display()))?;
        drop(file);
        std::fs::rename(temporary, &write_destination).with_context(|| {
            format!(
                "replace hook settings {} from {}",
                destination.display(),
                temporary.display()
            )
        })?;
        if let Some(parent) = write_destination.parent()
            && let Ok(directory) = std::fs::File::open(parent)
        {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

/// Whether a lifecycle artifact has to be replaced at all. Pure.
///
/// Every TUI launch reinstalls these artifacts, and replacing a byte-identical
/// file still gives it a new mtime — which trips Brain's own filesystem watcher
/// and uploads an unchanged hook script on every single startup. Comparing first
/// makes reinstallation genuinely idempotent on disk.
#[must_use]
pub(super) fn needs_rewrite(existing: Option<&[u8]>, contents: &str) -> bool {
    existing != Some(contents.as_bytes())
}

fn write_static_file(
    destination: &Path,
    write_destination: &Path,
    contents: &str,
    mode: u32,
) -> Result<()> {
    if !needs_rewrite(std::fs::read(write_destination).ok().as_deref(), contents) {
        return Ok(());
    }
    let parent = write_destination.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create lifecycle directory {}", parent.display()))?;
    let file_name = write_destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact");
    let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create lifecycle temporary {}", temporary.display()))?;
    let result = (|| {
        file.write_all(contents.as_bytes())
            .with_context(|| format!("write lifecycle temporary {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync lifecycle temporary {}", temporary.display()))?;
        drop(file);
        std::fs::rename(&temporary, write_destination).with_context(|| {
            format!(
                "replace lifecycle artifact {} from {}",
                destination.display(),
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

fn resolve_write_destination(path: &Path) -> Result<PathBuf> {
    let mut current = path.to_path_buf();
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(current),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect lifecycle path {}", current.display()));
            }
        };
        if !metadata.file_type().is_symlink() {
            return Ok(current);
        }
        anyhow::ensure!(
            visited.insert(current.clone()),
            "symlink cycle while installing {}",
            path.display()
        );
        let target = std::fs::read_link(&current)
            .with_context(|| format!("read lifecycle symlink {}", current.display()))?;
        current = if target.is_absolute() {
            target
        } else {
            current
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
    }
    anyhow::bail!(
        "symlink chain exceeds safe depth while installing {}",
        path.display()
    )
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("resolve current directory for lifecycle installation")?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                anyhow::ensure!(
                    normalized.pop(),
                    "lifecycle path escapes the filesystem root: {}",
                    path.display()
                );
            }
            std::path::Component::Normal(segment) => normalized.push(segment),
        }
    }
    Ok(normalized)
}

fn canonicalize_with_missing_tail(path: &Path) -> Result<PathBuf> {
    let mut current = normalize_absolute_path(path)?;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&current) {
            Ok(mut resolved) => {
                for segment in missing.iter().rev() {
                    resolved.push(segment);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::symlink_metadata(&current)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(error).with_context(|| {
                        format!("resolve lifecycle symlink {}", current.display())
                    });
                }
                let segment = current.file_name().ok_or_else(|| {
                    anyhow::anyhow!("no existing ancestor for lifecycle path {}", path.display())
                })?;
                missing.push(segment.to_os_string());
                anyhow::ensure!(
                    current.pop(),
                    "no existing ancestor for lifecycle path {}",
                    path.display()
                );
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("resolve lifecycle path {}", current.display()));
            }
        }
    }
}

fn resolve_confined_write_destination(path: &Path, workspace: &Path) -> Result<PathBuf> {
    let write_destination = resolve_write_destination(path)?;
    let resolved_workspace = canonicalize_with_missing_tail(workspace)?;
    let resolved_destination = canonicalize_with_missing_tail(&write_destination)?;
    anyhow::ensure!(
        resolved_destination.starts_with(&resolved_workspace),
        "lifecycle artifact {} resolves outside workspace {}",
        path.display(),
        workspace.display()
    );
    Ok(write_destination)
}

pub(super) fn install(root: &Path) -> Result<()> {
    let home = std::path::PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?,
    );
    install_for_home(root, &home)
}

fn install_for_home(root: &Path, home: &Path) -> Result<()> {
    install_for_home_with(root, home, |_| Ok(()))
}

pub(crate) fn lifecycle_installations() -> Vec<crate::agent::LifecycleInstallation> {
    crate::agent::registrations()
        .iter()
        .flat_map(|registration| registration.lifecycle().iter().copied())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LifecycleInstallStep {
    Directory(crate::agent::LifecycleInstallation),
    Lock(crate::agent::LifecycleInstallation),
    Artifact(crate::agent::LifecycleInstallation),
}

pub(super) fn install_for_home_with(
    root: &Path,
    home: &Path,
    mut after_step: impl FnMut(LifecycleInstallStep) -> Result<()>,
) -> Result<()> {
    for installation in lifecycle_installations() {
        let path = installation.path(root, home);
        let payload = installation.payload();
        let static_destination = match payload {
            crate::agent::LifecyclePayload::StaticFile { .. } => {
                Some(resolve_confined_write_destination(&path, root)?)
            }
            crate::agent::LifecyclePayload::HookSettings { .. } => None,
        };
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create lifecycle directory {}", parent.display()))?;
        after_step(LifecycleInstallStep::Directory(installation))?;
        match payload {
            crate::agent::LifecyclePayload::StaticFile { contents, mode } => {
                write_static_file(
                    &path,
                    static_destination
                        .as_deref()
                        .expect("static lifecycle payload has a resolved destination"),
                    contents,
                    mode,
                )?;
            }
            crate::agent::LifecyclePayload::HookSettings {
                style,
                session_script,
                completion_script,
                legacy_session_scripts,
                legacy_completion_scripts,
            } => {
                let session_path = root.join(session_script);
                let stop_path = root.join(completion_script);
                let (session, stop) = match style {
                    crate::agent::HookCommandStyle::WorkspaceRelative => {
                        (command(&session_path, root), command(&stop_path, root))
                    }
                    crate::agent::HookCommandStyle::PortableBrainRoot => (
                        portable_root_command(Path::new(session_script)),
                        portable_root_command(Path::new(completion_script)),
                    ),
                };
                update_json_file_with_temporary_and_lock(
                    &path,
                    &hook_temporary_path(&path),
                    |settings| {
                        let mut session_basenames = legacy_session_scripts.to_vec();
                        session_basenames.push(
                            Path::new(session_script)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .expect("registered session script has a UTF-8 basename"),
                        );
                        let mut completion_basenames = legacy_completion_scripts.to_vec();
                        completion_basenames.push(
                            Path::new(completion_script)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .expect("registered completion script has a UTF-8 basename"),
                        );
                        replace_entry(settings, "SessionStart", &session_basenames, &session);
                        replace_entry(settings, "Stop", &completion_basenames, &stop);
                    },
                    || after_step(LifecycleInstallStep::Lock(installation)),
                )?;
            }
        }
        after_step(LifecycleInstallStep::Artifact(installation))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
