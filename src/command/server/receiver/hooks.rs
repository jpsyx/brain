//! Workspace-sensitive Claude and Codex hook installation.

use std::path::Path;

use anyhow::Result;

fn ensure_entry(settings: &mut serde_json::Value, event: &str, command: &str) {
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
    let exists = list.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("command").and_then(serde_json::Value::as_str) == Some(command)
                })
            })
    });
    if !exists {
        list.push(serde_json::json!({"hooks": [{"type": "command", "command": command}]}));
    }
}

fn command(hook_path: &Path, root: &Path) -> String {
    hook_path.strip_prefix(root).map_or_else(
        |_| format!("python3 {}", hook_path.to_string_lossy()),
        |relative| format!("python3 {}", relative.to_string_lossy()),
    )
}

pub(super) fn install(root: &Path) -> Result<()> {
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
    std::fs::write(
        &stop_path,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/claude_stop_hook.py"
        )),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&session_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&stop_path, std::fs::Permissions::from_mode(0o755))?;
    }
    let home = std::path::PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?,
    );
    let session = command(&session_path, root);
    let stop = command(&stop_path, root);
    let settings_path = root.join(".claude/settings.json");
    let mut settings = if settings_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&settings_path)?)?
    } else {
        serde_json::json!({})
    };
    ensure_entry(&mut settings, "SessionStart", &session);
    ensure_entry(&mut settings, "Stop", &stop);
    std::fs::write(settings_path, serde_json::to_vec_pretty(&settings)?)?;
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    let codex_hooks_path = codex_dir.join("hooks.json");
    let mut codex_hooks = if codex_hooks_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&codex_hooks_path)?)?
    } else {
        serde_json::json!({})
    };
    ensure_entry(&mut codex_hooks, "SessionStart", &session);
    ensure_entry(&mut codex_hooks, "Stop", &stop);
    std::fs::write(codex_hooks_path, serde_json::to_vec_pretty(&codex_hooks)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{command, ensure_entry};

    #[test]
    fn command_is_project_relative_for_paths_under_the_selected_root() {
        let command = command(
            Path::new("/Users/pablo/family/.claude/brain-hooks/claude_stop_hook.py"),
            Path::new("/Users/pablo/family"),
        );
        assert_eq!(command, "python3 .claude/brain-hooks/claude_stop_hook.py");
    }

    #[test]
    fn command_falls_back_to_absolute_outside_the_selected_root() {
        assert_eq!(
            command(
                Path::new("/opt/hooks/x.py"),
                Path::new("/Users/pablo/family")
            ),
            "python3 /opt/hooks/x.py"
        );
    }

    #[test]
    fn project_relative_command_is_identical_across_workspace_roots() {
        let mini = command(
            Path::new("/Users/pablo/family/.claude/brain-hooks/claude_stop_hook.py"),
            Path::new("/Users/pablo/family"),
        );
        let mbp = command(
            Path::new(
                "/Users/juanpablosarmiento/fam-brain/.claude/brain-hooks/claude_stop_hook.py",
            ),
            Path::new("/Users/juanpablosarmiento/fam-brain"),
        );
        assert_eq!(mini, mbp);
    }

    #[test]
    fn merge_is_idempotent_and_preserves_other_settings() {
        let mut settings = json!({"permissions": {"allow": ["Read"]}});
        ensure_entry(&mut settings, "SessionStart", "/tmp/session.py");
        ensure_entry(&mut settings, "SessionStart", "/tmp/session.py");
        assert_eq!(
            settings["hooks"]["SessionStart"].as_array().unwrap().len(),
            1
        );
        assert_eq!(settings["permissions"]["allow"][0], "Read");
    }
}
