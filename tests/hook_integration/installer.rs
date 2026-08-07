use super::*;

fn settings_hook_commands(settings_path: &Path, event: &str) -> Vec<String> {
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(settings_path).expect("read installed settings"))
            .expect("parse installed settings");
    settings["hooks"][event]
        .as_array()
        .expect("hook event array")
        .iter()
        .flat_map(|entry| {
            entry["hooks"]
                .as_array()
                .expect("hook command array")
                .iter()
        })
        .map(|hook| {
            hook["command"]
                .as_str()
                .expect("hook command string")
                .to_owned()
        })
        .collect()
}

fn read_settings(settings_path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(settings_path).expect("read installed settings"))
        .expect("parse installed settings")
}

#[test]
fn installer_uses_the_explicit_selected_root_and_relative_project_commands() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family");
    let ignored_env_root = temp.path().join("ignored-env-root");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(selected_root.join(".claude")).expect("create settings directory");
    std::fs::write(
        selected_root.join(".claude/settings.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"python3 \"/old/claude_session_start_hook.py\""}]}],"Stop":[{"hooks":[{"type":"command","command":"python3 '/old/agent_turn_complete_hook.py'"}]}]}}"#,
    )
    .expect("write legacy settings");

    let output = Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh"))
        .arg(&selected_root)
        .env("HOME", &home)
        .env("BRAIN_ROOT", &ignored_env_root)
        .output()
        .expect("run hook installer");

    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let settings = selected_root.join(".claude/settings.json");
    assert_eq!(
        settings_hook_commands(&settings, "SessionStart"),
        vec!["python3 .claude/brain-hooks/agent_session_start_hook.py"]
    );
    assert_eq!(
        settings_hook_commands(&settings, "Stop"),
        vec!["python3 .claude/brain-hooks/agent_turn_complete_hook.py"]
    );
    for name in [
        "agent_session_start_hook.py",
        "agent_turn_complete_hook.py",
        "claude_session_start_hook.py",
        "claude_stop_hook.py",
    ] {
        assert!(
            selected_root
                .join(".claude/brain-hooks")
                .join(name)
                .is_file()
        );
    }
    assert_eq!(
        std::fs::read_to_string(selected_root.join(".opencode/plugins/brain.js")).unwrap(),
        include_str!("../../scripts/opencode_brain_plugin.js")
    );
    assert!(!ignored_env_root.exists());
    assert!(!home.join("brain").exists());
}

#[test]
fn installer_uses_brain_root_when_no_explicit_root_is_passed() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family-from-env");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh"))
        .env("HOME", &home)
        .env("BRAIN_ROOT", &selected_root)
        .output()
        .expect("run hook installer");

    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(selected_root.join(".claude/settings.json").is_file());
    assert!(!home.join("brain").exists());
}

#[test]
fn installer_reconciles_codex_hooks_idempotently() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family");
    let codex_hooks = home.join(".codex/hooks.json");
    std::fs::create_dir_all(codex_hooks.parent().expect("Codex settings parent"))
        .expect("create Codex settings directory");
    std::fs::write(
        &codex_hooks,
        r#"{
          "model": "custom-model",
          "hooks": {
            "PreToolUse": [{"hooks":[{"type":"command","command":"keep-pre-tool"}]}],
            "SessionStart": [
              {"hooks":[{"type":"command","command":"keep-session-start"}]},
              {"hooks":[{"type":"command","command":"python3 /stale/agent_session_start_hook.py"}]}
            ],
            "Stop": [
              {"hooks":[{"type":"command","command":"keep-stop"}]},
              {"hooks":[{"type":"command","command":"python3 '/stale/claude_stop_hook.py'"}]}
            ]
          }
        }"#,
    )
    .expect("write existing Codex settings");

    for _ in 0..2 {
        let output = Command::new("bash")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh"))
            .arg(&selected_root)
            .env("HOME", &home)
            .output()
            .expect("run hook installer");
        assert!(
            output.status.success(),
            "installer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(
        settings_hook_commands(&codex_hooks, "SessionStart"),
        vec![
            "keep-session-start",
            "python3 \"${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py\"",
        ]
    );
    assert_eq!(
        settings_hook_commands(&codex_hooks, "Stop"),
        vec![
            "keep-stop",
            "python3 \"${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py\"",
        ]
    );
    let settings = read_settings(&codex_hooks);
    assert_eq!(settings["model"], "custom-model");
    assert_eq!(
        settings_hook_commands(&codex_hooks, "PreToolUse"),
        vec!["keep-pre-tool"]
    );
}

#[cfg(unix)]
#[test]
fn installer_rejects_a_static_artifact_parent_symlink_outside_the_selected_root() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family");
    let outside_plugins = temp.path().join("outside-plugins");
    std::fs::create_dir_all(selected_root.join(".opencode")).expect("OpenCode directory");
    std::fs::create_dir_all(&outside_plugins).expect("outside plugin directory");
    std::fs::write(outside_plugins.join("brain.js"), "outside\n").expect("outside sentinel");
    std::os::unix::fs::symlink(&outside_plugins, selected_root.join(".opencode/plugins"))
        .expect("plugin parent symlink");

    let output = Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh"))
        .arg(&selected_root)
        .env("HOME", &home)
        .output()
        .expect("run hook installer");

    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(outside_plugins.join("brain.js")).unwrap(),
        "outside\n"
    );
}
