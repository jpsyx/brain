//! Workspace-sensitive Claude and Codex hook installation.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

fn replace_entry(
    settings: &mut serde_json::Value,
    event: &str,
    hook_basename: &str,
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
                    candidate
                        .trim_end_matches(['"', '\''])
                        .ends_with(hook_basename)
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

fn codex_command(hook_path: &Path) -> String {
    let hook_name = hook_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("hook paths have UTF-8 file names");
    format!(r#"python3 "${{BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/{hook_name}""#)
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

fn update_json_file(path: &Path, mutation: impl FnOnce(&mut serde_json::Value)) -> Result<()> {
    update_json_file_with_temporary(path, &hook_temporary_path(path), mutation)
}

fn update_json_file_with_temporary(
    path: &Path,
    temporary: &Path,
    mutation: impl FnOnce(&mut serde_json::Value),
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create hook settings directory {}", parent.display()))?;
    let lock_path = hook_lock_path(path);
    let connection = rusqlite::Connection::open(&lock_path)
        .with_context(|| format!("open hook settings lock {}", lock_path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .context("configure hook settings lock")?;
    connection
        .execute_batch("PRAGMA journal_mode = OFF; BEGIN IMMEDIATE")
        .with_context(|| format!("acquire hook settings lock {}", lock_path.display()))?;

    let mut settings = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("parse hook settings {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => {
            return Err(error).with_context(|| format!("read hook settings {}", path.display()));
        }
    };
    mutation(&mut settings);
    let mut bytes = serde_json::to_vec_pretty(&settings)
        .with_context(|| format!("serialize hook settings {}", path.display()))?;
    bytes.push(b'\n');

    write_and_replace_json(temporary, path, &bytes)
}

fn write_and_replace_json(temporary: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
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
        std::fs::rename(temporary, destination).with_context(|| {
            format!(
                "replace hook settings {} from {}",
                destination.display(),
                temporary.display()
            )
        })?;
        if let Some(parent) = destination.parent()
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

pub(super) fn install(root: &Path) -> Result<()> {
    let home = std::path::PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?,
    );
    install_for_home(root, &home)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstallStep {
    SessionScript,
    StopScript,
    ClaudeSettings,
    CodexSettings,
}

fn install_for_home(root: &Path, home: &Path) -> Result<()> {
    install_for_home_with(root, home, |_| Ok(()))
}

pub(super) fn install_for_home_with(
    root: &Path,
    home: &Path,
    mut after_write: impl FnMut(InstallStep) -> Result<()>,
) -> Result<()> {
    let hook_dir = root.join(".claude").join("brain-hooks");
    std::fs::create_dir_all(&hook_dir)?;
    let session_path = hook_dir.join("claude_session_start_hook.py");
    let stop_path = hook_dir.join("claude_stop_hook.py");
    std::fs::write(
        &session_path,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/claude_session_start_hook.py"
        )),
    )?;
    after_write(InstallStep::SessionScript)?;
    std::fs::write(
        &stop_path,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/claude_stop_hook.py"
        )),
    )?;
    after_write(InstallStep::StopScript)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&session_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&stop_path, std::fs::Permissions::from_mode(0o755))?;
    }
    let session = command(&session_path, root);
    let stop = command(&stop_path, root);
    let codex_session = codex_command(&session_path);
    let codex_stop = codex_command(&stop_path);
    let settings_path = root.join(".claude/settings.json");
    update_json_file(&settings_path, |settings| {
        replace_entry(
            settings,
            "SessionStart",
            "claude_session_start_hook.py",
            &session,
        );
        replace_entry(settings, "Stop", "claude_stop_hook.py", &stop);
    })?;
    after_write(InstallStep::ClaudeSettings)?;
    let codex_dir = home.join(".codex");
    let codex_hooks_path = codex_dir.join("hooks.json");
    update_json_file(&codex_hooks_path, |codex_hooks| {
        replace_entry(
            codex_hooks,
            "SessionStart",
            "claude_session_start_hook.py",
            &codex_session,
        );
        replace_entry(codex_hooks, "Stop", "claude_stop_hook.py", &codex_stop);
    })?;
    after_write(InstallStep::CodexSettings)?;
    Ok(())
}

#[cfg(test)]
mod tests;
