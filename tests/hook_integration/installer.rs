use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

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

fn run_installed_observation_bridge(
    selected_root: &Path,
    observation: &Path,
    payload: &serde_json::Value,
) -> std::process::Output {
    let mut child = Command::new("python3")
        .arg(selected_root.join(".brain/hooks/receiver_observation_bridge.py"))
        .arg("--require-write")
        .env(
            "BRAIN_RECEIVER_JOB_TOKEN",
            "11111111-1111-4111-8111-111111111111",
        )
        .env("BRAIN_INSTANCE_ID", "22222222-2222-4222-8222-222222222222")
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", observation)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run installed observation bridge");
    child
        .stdin
        .take()
        .expect("installed bridge stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write installed bridge payload");
    child.wait_with_output().expect("wait installed bridge")
}

#[test]
fn installer_self_heals_repeated_progress_artifacts() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family");
    let hooks = selected_root.join(".brain/hooks");
    let plugins = selected_root.join(".opencode/plugins");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&hooks).expect("create hooks");
    std::fs::create_dir_all(&plugins).expect("create plugins");
    std::fs::write(hooks.join("receiver_observation_bridge.py"), "stale\n").expect("stale bridge");
    std::fs::write(plugins.join("brain.js"), "stale\n").expect("stale plugin");
    let selected_root = std::fs::canonicalize(selected_root).expect("canonical selected root");

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
    assert_eq!(
        std::fs::read_to_string(plugins.join("brain.js")).unwrap(),
        include_str!("../../scripts/opencode_brain_plugin.js")
    );

    let cache = selected_root.join("cache");
    let observations = cache.join("receiver-observations");
    std::fs::create_dir_all(&observations).expect("observation directories");
    #[cfg(unix)]
    for directory in [&cache, &observations] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .expect("owner-only observation directory");
    }
    let observation = observations.join("observation.json");
    let marker = "<!-- brain:receiver-job-token=11111111-1111-4111-8111-111111111111 -->";
    let submit = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "installed-session",
        "prompt": marker,
    });
    let accepted = run_installed_observation_bridge(&selected_root, &observation, &submit);
    assert!(
        accepted.status.success(),
        "installed acceptance failed: {accepted:?}"
    );
    for turn_id in ["turn-1", "turn-2"] {
        std::thread::sleep(std::time::Duration::from_millis(2));
        let progress = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "session_id": "installed-session",
            "turn_id": turn_id,
        });
        assert!(
            run_installed_observation_bridge(&selected_root, &observation, &progress)
                .status
                .success()
        );
    }
    let snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(observation).unwrap()).unwrap();
    assert_eq!(snapshot["revision"], 3);
    assert_eq!(snapshot["turn_id"], "turn-2");
    assert!(snapshot["latest_progress_at_unix_ms"].as_u64().is_some());
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
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"python3 ~/brain/.claude/brain-hooks/claude_session_start_hook.py"}]},{"hooks":[{"type":"command","command":"python3 /opt/user/claude_session_start_hook.py"}]}],"Stop":[{"hooks":[{"type":"command","command":"python3 ~/brain/.claude/brain-hooks/claude_stop_hook.py"}]},{"hooks":[{"type":"command","command":"python3 /opt/user/claude_stop_hook.py"}]}]}}"#,
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
        vec![
            "python3 /opt/user/claude_session_start_hook.py",
            r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_start_hook.py""#
        ]
    );
    assert_eq!(
        settings_hook_commands(&settings, "Stop"),
        vec![
            "python3 /opt/user/claude_stop_hook.py",
            r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_stop_hook.py""#
        ]
    );
    for name in [
        "agent_session_start_hook.py",
        "agent_session_stop_hook.py",
        "receiver_observation_bridge.py",
    ] {
        assert!(selected_root.join(".brain/hooks").join(name).is_file());
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
    let codex_hooks = selected_root.join(".codex/hooks.json");
    std::fs::create_dir_all(codex_hooks.parent().expect("Codex settings parent"))
        .expect("create Codex settings directory");
    std::fs::write(
        &codex_hooks,
        r#"{
          "model": "custom-model",
          "hooks": {
            "PreToolUse": [{"hooks":[{"type":"command","command":"keep-pre-tool"}]}],
            "UserPromptSubmit": [
              {"hooks":[{"type":"command","command":"keep-prompt"}]},
              {"hooks":[{"type":"command","command":"python3 /opt/user/receiver_observation_bridge.py"}]}
            ],
            "PostToolUse": [
              {"hooks":[{"type":"command","command":"keep-post-tool"}]},
              {"hooks":[{"type":"command","command":"python3 /opt/user/receiver_observation_bridge.py"}]}
            ],
            "SessionStart": [
              {"hooks":[{"type":"command","command":"keep-session-start"}]},
              {"hooks":[{"type":"command","command":"python3 /opt/user/agent_session_start_hook.py"}]},
              {"hooks":[{"type":"command","command":"python3 \"${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py\""}]}
            ],
            "Stop": [
              {"hooks":[{"type":"command","command":"keep-stop"}]},
              {"hooks":[{"type":"command","command":"python3 /opt/user/agent_session_stop_hook.py"}]},
              {"hooks":[{"type":"command","command":"python3 \"${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py\""}]}
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
            "python3 /opt/user/agent_session_start_hook.py",
            "python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py\"",
        ]
    );
    assert_eq!(
        settings_hook_commands(&codex_hooks, "Stop"),
        vec![
            "keep-stop",
            "python3 /opt/user/agent_session_stop_hook.py",
            "python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py\"",
        ]
    );
    let settings = read_settings(&codex_hooks);
    assert_eq!(settings["model"], "custom-model");
    assert_eq!(
        settings_hook_commands(&codex_hooks, "PreToolUse"),
        vec!["keep-pre-tool"]
    );
    assert_eq!(
        settings_hook_commands(&codex_hooks, "UserPromptSubmit"),
        vec![
            "keep-prompt",
            "python3 /opt/user/receiver_observation_bridge.py",
            "python3 \"${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py\"",
        ]
    );
    assert_eq!(
        settings_hook_commands(&codex_hooks, "PostToolUse"),
        vec![
            "keep-post-tool",
            "python3 /opt/user/receiver_observation_bridge.py",
            "python3 \"${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py\"",
        ]
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
