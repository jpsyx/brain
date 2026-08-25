use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const BRAIN_HOOK_COMMANDS: &[&str] = &[
    r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_start_hook.py""#,
    r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_stop_hook.py""#,
    r#"python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py""#,
    r#"python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py""#,
    "python3 .brain/hooks/agent_session_start_hook.py",
    "python3 .brain/hooks/agent_session_stop_hook.py",
    r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/claude_session_start_hook.py""#,
    r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_session_start_hook.py""#,
    r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/claude_stop_hook.py""#,
    r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_turn_complete_hook.py""#,
    r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/claude_session_start_hook.py""#,
    r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py""#,
    r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/claude_stop_hook.py""#,
    r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py""#,
    "python3 .claude/brain-hooks/claude_session_start_hook.py",
    "python3 .claude/brain-hooks/agent_session_start_hook.py",
    "python3 .claude/brain-hooks/claude_stop_hook.py",
    "python3 .claude/brain-hooks/agent_turn_complete_hook.py",
];

const SESSION_START_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/agent_session_start_hook.py"
));
const SESSION_STOP_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/agent_session_stop_hook.py"
));
const CLAUDE_SESSION_START_SCRIPT: &str = concat!(
    "#!/usr/bin/env python3\n",
    "\"\"\"Compatibility launcher for Brain's generic session-start bridge.\"\"\"\n\n",
    "from pathlib import Path\n",
    "import runpy\n\n\n",
    "runpy.run_path(\n",
    "    str(Path(__file__).with_name(\"agent_session_start_hook.py\")),\n",
    "    run_name=\"__main__\",\n",
    ")\n",
);
const CLAUDE_STOP_SCRIPT: &str = concat!(
    "#!/usr/bin/env python3\n",
    "\"\"\"Compatibility launcher for Brain's generic turn-complete bridge.\"\"\"\n\n",
    "from pathlib import Path\n",
    "import runpy\n\n\n",
    "runpy.run_path(\n",
    "    str(Path(__file__).with_name(\"agent_turn_complete_hook.py\")),\n",
    "    run_name=\"__main__\",\n",
    ")\n",
);
const SESSION_START_SHIM: &str = concat!(
    "#!/usr/bin/env python3\n",
    "\"\"\"Forward a hook cached by a frontend that predates Brain 0.71.\"\"\"\n\n",
    "from pathlib import Path\n",
    "import runpy\n\n\n",
    "runpy.run_path(\n",
    "    str(Path(__file__).parents[2] / \".brain/hooks/agent_session_start_hook.py\"),\n",
    "    run_name=\"__main__\",\n",
    ")\n",
);
const SESSION_STOP_SHIM: &str = concat!(
    "#!/usr/bin/env python3\n",
    "\"\"\"Forward a hook cached by a frontend that predates Brain 0.71.\"\"\"\n\n",
    "from pathlib import Path\n",
    "import runpy\n\n\n",
    "runpy.run_path(\n",
    "    str(Path(__file__).parents[2] / \".brain/hooks/agent_session_stop_hook.py\"),\n",
    "    run_name=\"__main__\",\n",
    ")\n",
);
const OPENCODE_PLUGIN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/opencode_brain_plugin.js"
));

pub(super) fn up(home: &Path) -> Result<()> {
    remove_global_json_hooks(&home.join(".claude/settings.json"))?;
    remove_global_json_hooks(&home.join(".codex/hooks.json"))?;
    remove_global_opencode_plugins(home)?;
    install_workspace_hooks()
}

pub(super) fn down(home: &Path) -> Result<()> {
    remove_global_json_hooks(&home.join(".claude/settings.json"))?;
    remove_global_opencode_plugins(home)?;
    let roots = workspace_roots();
    for root in &roots {
        remove_current_workspace_scripts(root)?;
        remove_workspace_codex_hooks(root)?;
        restore_previous_workspace_lifecycle(root)?;
    }
    restore_previous_global_codex_hooks(&home.join(".codex/hooks.json"))
}

fn remove_global_json_hooks(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    crate::command::server::update_agent_hook_json(path, |settings| {
        for event in ["SessionStart", "Stop"] {
            remove_entries(settings, event);
        }
        prune_empty_hooks(settings);
    })
    .with_context(|| format!("remove global Brain hooks from {}", path.display()))
}

fn remove_entries(settings: &mut serde_json::Value, event: &str) {
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
            !hook
                .get("command")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_brain_hook_command)
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

fn is_brain_hook_command(command: &str) -> bool {
    BRAIN_HOOK_COMMANDS.contains(&command)
}

fn remove_global_opencode_plugins(home: &Path) -> Result<()> {
    let mut candidates = vec![home.join(".config/opencode/plugins/brain.js")];
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(xdg).join("opencode/plugins/brain.js"));
    }
    candidates.push(home.join(".opencode/plugins/brain.js"));
    candidates.sort();
    candidates.dedup();
    for path in candidates {
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if contents.contains("Brain lifecycle bridge for OpenCode") {
            std::fs::remove_file(&path)
                .with_context(|| format!("remove global OpenCode plugin {}", path.display()))?;
        }
    }
    Ok(())
}

fn install_workspace_hooks() -> Result<()> {
    for root in workspace_roots() {
        crate::command::server::refresh_agent_hooks(&root)
            .with_context(|| format!("install workspace hooks in {}", root.display()))?;
        install_active_session_shims(&root)?;
    }
    Ok(())
}

pub(super) fn workspace_roots() -> Vec<PathBuf> {
    let path = crate::workspace::RegistryStore::real().path().to_path_buf();
    let registry = match crate::workspace::RegistryStore::load_readable(&path) {
        Ok(registry) => registry,
        Err(crate::workspace::RegistryError::Io {
            kind: std::io::ErrorKind::NotFound,
            ..
        }) => {
            return Vec::new();
        }
        Err(_) => {
            let legacy_root = crate::paths::brain_root_path();
            return legacy_root
                .is_dir()
                .then_some(legacy_root)
                .into_iter()
                .collect();
        }
    };
    registry
        .workspaces
        .values()
        .filter(|record| record.root.is_dir())
        .map(|record| record.root.clone())
        .collect()
}

fn install_active_session_shims(root: &Path) -> Result<()> {
    for (relative, contents) in [
        (
            ".claude/brain-hooks/claude_session_start_hook.py",
            SESSION_START_SHIM,
        ),
        (
            ".claude/brain-hooks/agent_session_start_hook.py",
            SESSION_START_SHIM,
        ),
        (".claude/brain-hooks/claude_stop_hook.py", SESSION_STOP_SHIM),
        (
            ".claude/brain-hooks/agent_turn_complete_hook.py",
            SESSION_STOP_SHIM,
        ),
    ] {
        crate::command::server::write_agent_workspace_artifact(
            root,
            Path::new(relative),
            contents,
            0o755,
        )?;
    }
    Ok(())
}

fn remove_current_workspace_scripts(root: &Path) -> Result<()> {
    let directory = root.join(".brain/hooks");
    for name in ["agent_session_start_hook.py", "agent_session_stop_hook.py"] {
        remove_file_if_present(&directory.join(name))?;
    }
    remove_directory_if_empty(&directory)?;
    if let Some(parent) = directory.parent() {
        remove_directory_if_empty(parent)?;
    }
    Ok(())
}

fn remove_workspace_codex_hooks(root: &Path) -> Result<()> {
    let path = root.join(".codex/hooks.json");
    if !path.exists() {
        return Ok(());
    }
    crate::command::server::update_agent_hook_json(&path, |settings| {
        for event in ["SessionStart", "Stop"] {
            remove_entries(settings, event);
        }
        prune_empty_hooks(settings);
    })?;
    let settings: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    if settings.as_object().is_some_and(serde_json::Map::is_empty) {
        remove_file_if_present(&path)?;
    }
    Ok(())
}

fn restore_previous_workspace_lifecycle(root: &Path) -> Result<()> {
    for (relative, contents) in [
        (
            ".claude/brain-hooks/agent_session_start_hook.py",
            SESSION_START_SCRIPT,
        ),
        (
            ".claude/brain-hooks/agent_turn_complete_hook.py",
            SESSION_STOP_SCRIPT,
        ),
        (
            ".claude/brain-hooks/claude_session_start_hook.py",
            CLAUDE_SESSION_START_SCRIPT,
        ),
        (
            ".claude/brain-hooks/claude_stop_hook.py",
            CLAUDE_STOP_SCRIPT,
        ),
    ] {
        crate::command::server::write_agent_workspace_artifact(
            root,
            Path::new(relative),
            contents,
            0o755,
        )?;
    }
    let claude_settings = root.join(".claude/settings.json");
    set_hook_entries(
        &claude_settings,
        r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_session_start_hook.py""#,
        r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_turn_complete_hook.py""#,
    )?;
    let legacy_plugin = OPENCODE_PLUGIN
        .replace(".brain/hooks", ".claude/brain-hooks")
        .replace("agent_session_stop_hook.py", "agent_turn_complete_hook.py")
        .replace("session_stop_bridge", "turn_complete_bridge");
    crate::command::server::write_agent_workspace_artifact(
        root,
        Path::new(".opencode/plugins/brain.js"),
        &legacy_plugin,
        0o644,
    )
}

fn restore_previous_global_codex_hooks(path: &Path) -> Result<()> {
    set_hook_entries(
        path,
        r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py""#,
        r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py""#,
    )
}

fn set_hook_entries(path: &Path, session_start: &str, stop: &str) -> Result<()> {
    crate::command::server::update_agent_hook_json(path, |settings| {
        for event in ["SessionStart", "Stop"] {
            remove_entries(settings, event);
        }
        add_entry(settings, "SessionStart", session_start);
        add_entry(settings, "Stop", stop);
    })
}

fn add_entry(settings: &mut serde_json::Value, event: &str, command: &str) {
    let root = settings
        .as_object_mut()
        .expect("hook settings root is an object");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("hooks is an object");
    hooks
        .entry(event)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("hook event is an array")
        .push(serde_json::json!({
            "hooks": [{"type": "command", "command": command}]
        }));
}

fn remove_file_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("remove lifecycle artifact {}", path.display()))
        }
    }
}

fn remove_directory_if_empty(path: &Path) -> Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => {
            Err(error).with_context(|| format!("remove lifecycle directory {}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_identifies_only_exact_current_and_known_legacy_brain_commands() {
        assert!(is_brain_hook_command(
            r#"python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py""#
        ));
        assert!(is_brain_hook_command(
            "python3 .claude/brain-hooks/claude_session_start_hook.py"
        ));
        assert!(!is_brain_hook_command(
            "python3 /opt/user/agent_session_stop_hook.py"
        ));
        assert!(!is_brain_hook_command("python3 /tmp/unrelated.py"));
    }
}
