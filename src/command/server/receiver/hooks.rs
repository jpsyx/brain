//! Workspace-sensitive lifecycle integration for configured agent frontends.

use std::path::Path;

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

/// Claude's hook command, anchored to the project root rather than the cwd.
///
/// Claude runs a hook in the session's *current* working directory, and its
/// Bash tool's `cd` persists, so a project-relative command silently stops
/// resolving as soon as an agent changes directory. `CLAUDE_PROJECT_DIR` is the
/// project root Claude itself exports for exactly this; `BRAIN_ROOT` covers a
/// session Brain launched. No machine-specific absolute path is written,
/// because the settings file is synced and read on every machine.
fn claude_project_dir_command(hook_path: &Path) -> String {
    let relative = hook_path
        .to_string_lossy()
        .trim_start_matches('/')
        .to_owned();
    format!(r#"python3 "${{CLAUDE_PROJECT_DIR:-${{BRAIN_ROOT}}}}/{relative}""#)
}

fn portable_root_command(hook_path: &Path) -> String {
    let relative = hook_path
        .to_string_lossy()
        .trim_start_matches('/')
        .to_owned();
    format!(r#"python3 "${{BRAIN_ROOT}}/{relative}""#)
}

mod json;

pub(crate) use json::update_json_file;
#[cfg(test)]
use json::update_json_file_with_temporary;
use json::{hook_temporary_path, update_json_file_with_temporary_and_lock};

mod artifact;

#[cfg(test)]
use artifact::needs_rewrite;
pub(crate) use artifact::write_workspace_artifact;
use artifact::{resolve_confined_write_destination, write_static_file};

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
                observation_script,
                legacy_session_scripts,
                legacy_completion_scripts,
            } => {
                // Both styles name the script through a root variable, so no
                // command embeds this machine's resolved path.
                let (session, stop, observation) = match style {
                    crate::agent::HookCommandStyle::ClaudeProjectDir => (
                        claude_project_dir_command(Path::new(session_script)),
                        claude_project_dir_command(Path::new(completion_script)),
                        claude_project_dir_command(Path::new(observation_script)),
                    ),
                    crate::agent::HookCommandStyle::PortableBrainRoot => (
                        portable_root_command(Path::new(session_script)),
                        portable_root_command(Path::new(completion_script)),
                        portable_root_command(Path::new(observation_script)),
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
                        let observation_basename = Path::new(observation_script)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .expect("registered observation script has a UTF-8 basename");
                        replace_entry(
                            settings,
                            "UserPromptSubmit",
                            &[observation_basename],
                            &observation,
                        );
                        replace_entry(
                            settings,
                            "PostToolUse",
                            &[observation_basename],
                            &observation,
                        );
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
