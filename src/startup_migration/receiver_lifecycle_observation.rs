//! Upgrade and downgrade the receiver lifecycle evidence producers.

use std::path::Path;

use anyhow::{Context as _, Result};

const PREVIOUS_OPENCODE_PLUGIN: &str = include_str!("assets/opencode_brain_plugin_0_80.js");
const CLAUDE_OBSERVATION_COMMAND: &str = r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/receiver_observation_bridge.py""#;
const CODEX_OBSERVATION_COMMAND: &str = r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py""#;

pub(super) fn up(_home: &Path) -> Result<()> {
    for root in super::lifecycle::workspace_roots() {
        crate::command::server::refresh_agent_hooks(&root).with_context(|| {
            format!(
                "install receiver observation producers in {}",
                root.display()
            )
        })?;
    }
    Ok(())
}

pub(super) fn down(_home: &Path) -> Result<()> {
    for root in super::lifecycle::workspace_roots() {
        remove_observation_settings(
            &root.join(".claude/settings.json"),
            CLAUDE_OBSERVATION_COMMAND,
        )?;
        remove_observation_settings(&root.join(".codex/hooks.json"), CODEX_OBSERVATION_COMMAND)?;
        remove_if_present(&root.join(".brain/hooks/receiver_observation_bridge.py"))?;
        crate::command::server::write_agent_workspace_artifact(
            &root,
            Path::new(".opencode/plugins/brain.js"),
            PREVIOUS_OPENCODE_PLUGIN,
            0o644,
        )?;
    }
    Ok(())
}

fn remove_observation_settings(path: &Path, managed_command: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    crate::command::server::update_agent_hook_json(path, |settings| {
        for event in ["UserPromptSubmit", "PostToolUse"] {
            remove_observation_entries(settings, event, managed_command);
        }
        prune_empty_hooks(settings);
    })
    .with_context(|| format!("remove receiver observation hooks from {}", path.display()))
}

fn remove_observation_entries(
    settings: &mut serde_json::Value,
    event: &str,
    managed_command: &str,
) {
    let Some(entries) = settings
        .get_mut("hooks")
        .and_then(|hooks| hooks.get_mut(event))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    entries.retain_mut(|entry| {
        let Some(hooks) = entry
            .get_mut("hooks")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return true;
        };
        hooks.retain(|hook| {
            hook.get("command")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|command| command != managed_command)
        });
        !hooks.is_empty()
    });
}

fn prune_empty_hooks(settings: &mut serde_json::Value) {
    let Some(root) = settings.as_object_mut() else {
        return;
    };
    let Some(hooks) = root
        .get_mut("hooks")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    hooks.retain(|_, entries| entries.as_array().is_none_or(|entries| !entries.is_empty()));
    if hooks.is_empty() {
        root.remove("hooks");
    }
}

fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}
