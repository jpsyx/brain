//! Workspace-sensitive Claude and Codex hook installation.

use std::path::Path;

use anyhow::Result;

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
                .is_some_and(|candidate| candidate.ends_with(hook_basename))
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

pub(super) fn install(root: &Path) -> Result<()> {
    let home = std::path::PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?,
    );
    install_for_home(root, &home)
}

fn install_for_home(root: &Path, home: &Path) -> Result<()> {
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
    let session = command(&session_path, root);
    let stop = command(&stop_path, root);
    let settings_path = root.join(".claude/settings.json");
    let mut settings = if settings_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&settings_path)?)?
    } else {
        serde_json::json!({})
    };
    replace_entry(
        &mut settings,
        "SessionStart",
        "claude_session_start_hook.py",
        &session,
    );
    replace_entry(&mut settings, "Stop", "claude_stop_hook.py", &stop);
    std::fs::write(settings_path, serde_json::to_vec_pretty(&settings)?)?;
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    let codex_hooks_path = codex_dir.join("hooks.json");
    let mut codex_hooks = if codex_hooks_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&codex_hooks_path)?)?
    } else {
        serde_json::json!({})
    };
    replace_entry(
        &mut codex_hooks,
        "SessionStart",
        "claude_session_start_hook.py",
        &session,
    );
    replace_entry(&mut codex_hooks, "Stop", "claude_stop_hook.py", &stop);
    std::fs::write(codex_hooks_path, serde_json::to_vec_pretty(&codex_hooks)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use serde_json::json;

    use super::{command, install_for_home, replace_entry};

    fn configured_command(path: &Path, event: &str) -> String {
        let settings: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        settings["hooks"][event][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn run_configured(
        root: &Path,
        command: &str,
        env: &[(&str, &std::ffi::OsStr)],
        input: &serde_json::Value,
    ) -> std::process::Output {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(root)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }

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
        replace_entry(
            &mut settings,
            "SessionStart",
            "session.py",
            "/tmp/session.py",
        );
        replace_entry(
            &mut settings,
            "SessionStart",
            "session.py",
            "/tmp/session.py",
        );
        assert_eq!(
            settings["hooks"]["SessionStart"].as_array().unwrap().len(),
            1
        );
        assert_eq!(settings["permissions"]["allow"][0], "Read");
    }

    #[test]
    fn installed_codex_start_and_stop_hooks_complete_one_attributed_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let root = temp.path().join("brain");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let stale_dir = root.join(".claude/brain-hooks");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::write(stale_dir.join("claude_session_start_hook.py"), "# stale\n").unwrap();
        std::fs::write(stale_dir.join("claude_stop_hook.py"), "# stale\n").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            root.join(".claude/settings.json"),
            serde_json::to_vec(&serde_json::json!({
                "hooks": {
                    "SessionStart": [{"hooks": [{"type": "command", "command": "python3 /old/claude_session_start_hook.py"}]}],
                    "Stop": [{"hooks": [{"type": "command", "command": "python3 /old/claude_stop_hook.py"}] }]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex/hooks.json"),
            serde_json::to_vec(&serde_json::json!({
                "hooks": {
                    "SessionStart": [{"hooks": [{"type": "command", "command": "python3 /old/claude_session_start_hook.py"}]}],
                    "Stop": [{"hooks": [{"type": "command", "command": "python3 /old/claude_stop_hook.py"}] }]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        install_for_home(&root, &home).unwrap();
        let codex_hooks = home.join(".codex/hooks.json");
        let start = configured_command(&codex_hooks, "SessionStart");
        let stop = configured_command(&codex_hooks, "Stop");
        let db_path = temp.path().join("state.db");
        drop(crate::state::Db::open_path(&db_path).unwrap());
        let response_dir = temp.path().join("responses");
        let common = [
            (
                "BRAIN_WORKSPACE_ID",
                std::ffi::OsStr::new("11111111-1111-4111-8111-111111111111"),
            ),
            ("BRAIN_WORKSPACE", std::ffi::OsStr::new("brain")),
            ("BRAIN_ROOT", root.as_os_str()),
            ("BRAIN_ACTOR_ID", std::ffi::OsStr::new("pablo")),
            ("BRAIN_CHANNEL", std::ffi::OsStr::new("interactive")),
            ("BRAIN_AGENT_KIND", std::ffi::OsStr::new("codex")),
            ("BRAIN_INSTANCE_ID", std::ffi::OsStr::new("instance-1")),
            ("BRAIN_PID", std::ffi::OsStr::new("4242")),
            ("BRAIN_STATE_DB", db_path.as_os_str()),
            ("BRAIN_RESPONSE_DIR", response_dir.as_os_str()),
            ("BRAIN_RESPONSE_ID", std::ffi::OsStr::new("response-1")),
        ];

        let started = run_configured(
            &root,
            &start,
            &common,
            &serde_json::json!({"thread_id":"codex-thread-1","source":"startup"}),
        );
        let stopped = run_configured(
            &root,
            &stop,
            &common,
            &serde_json::json!({
                "thread_id":"codex-thread-1",
                "last_assistant_message":"Finished"
            }),
        );

        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        assert!(
            stopped.status.success(),
            "{}",
            String::from_utf8_lossy(&stopped.stderr)
        );
        let connection = rusqlite::Connection::open(db_path).unwrap();
        let attribution = connection
            .query_row(
                "SELECT agent_kind, actor_id, channel FROM brain_sessions
                 WHERE agent_session_id = 'codex-thread-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            attribution,
            (
                "codex".to_owned(),
                "pablo".to_owned(),
                "interactive".to_owned()
            )
        );
        let response: serde_json::Value =
            serde_json::from_slice(&std::fs::read(response_dir.join("response-1.json")).unwrap())
                .unwrap();
        assert_eq!(response["session_id"], "codex-thread-1");
        assert_eq!(response["actor_id"], "pablo");
        assert_eq!(response["channel"], "interactive");
    }
}
