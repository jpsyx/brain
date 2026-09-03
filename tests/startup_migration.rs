use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const BRAIN: &str = env!("CARGO_BIN_EXE_brain");

fn workspace_record(root: &Path, workspace_id: &str) -> Value {
    json!({
        "workspace_id": workspace_id,
        "root": root,
        "aliases": [],
        "local_user_id": "pablo",
        "receiver_enabled": false,
        "env": {}
    })
}

struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    xdg_config: PathBuf,
    family: PathBuf,
    work: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary root");
        let home = temporary.path().join("home");
        let xdg_config = home.join(".config");
        let family = temporary.path().join("family");
        let work = temporary.path().join("work");
        for path in [&home, &family, &work] {
            std::fs::create_dir_all(path).expect("fixture directory");
        }
        std::fs::create_dir_all(xdg_config.join("brain")).expect("brain config directory");
        std::fs::write(
            xdg_config.join("brain/env.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 4,
                "default_workspace": "family",
                "workspaces": {
                    "family": workspace_record(
                        &family,
                        "11111111-1111-4111-8111-111111111111"
                    ),
                    "work": workspace_record(
                        &work,
                        "22222222-2222-4222-8222-222222222222"
                    )
                }
            }))
            .expect("registry JSON"),
        )
        .expect("registry");
        Self {
            _temporary: temporary,
            home,
            xdg_config,
            family,
            work,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(BRAIN)
            .args(arguments)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg_config)
            .output()
            .expect("run brain")
    }
}

fn configured_commands(path: &Path, event: &str) -> Vec<String> {
    let settings: Value = serde_json::from_slice(&std::fs::read(path).expect("settings bytes"))
        .expect("settings JSON");
    settings
        .get("hooks")
        .and_then(|hooks| hooks.get(event))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| entry["hooks"].as_array().expect("hook list"))
        .filter_map(|hook| hook["command"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn ordinary_startup_removes_global_hooks_and_installs_every_workspace_frontend() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.home.join(".claude")).expect("Claude config");
    std::fs::write(
        fixture.home.join(".claude/settings.json"),
        serde_json::to_vec_pretty(&json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_session_start_hook.py\""}]},
                    {"hooks": [{"type": "command", "command": "python3 ~/brain/.claude/brain-hooks/claude_session_start_hook.py"}]},
                    {"hooks": [{"type": "command", "command": "python3 /opt/user/agent_session_start_hook.py"}]},
                    {"hooks": [{"type": "command", "command": "python3 /opt/user/claude_session_start_hook.py"}]},
                    {"hooks": [{"type": "command", "command": "python3 /keep/claude.py"}]}
                ],
                "Stop": [
                    {"hooks": [{"type": "command", "command": "python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_turn_complete_hook.py\""}]},
                    {"hooks": [{"type": "command", "command": "python3 ~/brain/.claude/brain-hooks/claude_stop_hook.py"}]},
                    {"hooks": [{"type": "command", "command": "python3 /opt/user/claude_stop_hook.py"}]}
                ]
            },
            "permissions": {"allow": ["Read"]}
        }))
        .expect("Claude settings"),
    )
    .expect("write Claude settings");
    std::fs::create_dir_all(fixture.home.join(".codex")).expect("Codex config");
    std::fs::write(
        fixture.home.join(".codex/hooks.json"),
        serde_json::to_vec_pretty(&json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "python3 \"${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py\""}]}
                ],
                "Stop": [
                    {"hooks": [{"type": "command", "command": "python3 \"${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py\""}]},
                    {"hooks": [{"type": "command", "command": "python3 /opt/user/agent_session_stop_hook.py"}]},
                    {"hooks": [{"type": "command", "command": "python3 /keep/codex.py"}]}
                ]
            }
        }))
        .expect("Codex settings"),
    )
    .expect("write Codex settings");
    let global_opencode = fixture.xdg_config.join("opencode/plugins/brain.js");
    std::fs::create_dir_all(global_opencode.parent().expect("plugin parent"))
        .expect("OpenCode config");
    std::fs::write(
        &global_opencode,
        "// Brain lifecycle bridge for OpenCode.\nexport const BrainPlugin = async () => ({});\n",
    )
    .expect("global OpenCode plugin");

    let output = fixture.run(&["server", "status"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        configured_commands(&fixture.home.join(".claude/settings.json"), "SessionStart"),
        vec![
            "python3 /opt/user/agent_session_start_hook.py",
            "python3 /opt/user/claude_session_start_hook.py",
            "python3 /keep/claude.py",
        ]
    );
    assert_eq!(
        configured_commands(&fixture.home.join(".claude/settings.json"), "Stop"),
        vec!["python3 /opt/user/claude_stop_hook.py"]
    );
    assert!(
        configured_commands(&fixture.home.join(".codex/hooks.json"), "SessionStart").is_empty()
    );
    assert_eq!(
        configured_commands(&fixture.home.join(".codex/hooks.json"), "Stop"),
        vec![
            "python3 /opt/user/agent_session_stop_hook.py",
            "python3 /keep/codex.py",
        ]
    );
    assert!(!global_opencode.exists());

    for root in [&fixture.family, &fixture.work] {
        let scripts = root.join(".brain/hooks");
        assert!(scripts.join("agent_session_start_hook.py").is_file());
        assert!(scripts.join("agent_session_stop_hook.py").is_file());
        assert!(root.join(".opencode/plugins/brain.js").is_file());
        assert_eq!(
            configured_commands(&root.join(".claude/settings.json"), "SessionStart"),
            vec![
                r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_start_hook.py""#
            ]
        );
        assert_eq!(
            configured_commands(&root.join(".claude/settings.json"), "Stop"),
            vec![
                r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_stop_hook.py""#
            ]
        );
        assert_eq!(
            configured_commands(&root.join(".codex/hooks.json"), "SessionStart"),
            vec![
                r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py""#
            ]
        );
        assert_eq!(
            configured_commands(&root.join(".codex/hooks.json"), "Stop"),
            vec![
                r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py""#
            ]
        );
    }
}

#[test]
fn ordinary_startup_replaces_superseded_workspace_hooks_with_live_session_shims() {
    let fixture = Fixture::new();
    for root in [&fixture.family, &fixture.work] {
        let old_directory = root.join(".claude/brain-hooks");
        std::fs::create_dir_all(&old_directory).expect("old hook directory");
        for name in [
            "claude_session_start_hook.py",
            "claude_stop_hook.py",
            "agent_session_start_hook.py",
            "agent_turn_complete_hook.py",
        ] {
            std::fs::write(old_directory.join(name), "# old Brain hook\n")
                .expect("old hook script");
        }
    }

    let output = fixture.run(&["server", "status"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for root in [&fixture.family, &fixture.work] {
        std::fs::write(
            root.join(".brain/hooks/agent_session_start_hook.py"),
            "raise SystemExit(19)\n",
        )
        .expect("replace session-start target");
        std::fs::write(
            root.join(".brain/hooks/agent_session_stop_hook.py"),
            "raise SystemExit(23)\n",
        )
        .expect("replace session-stop target");
        for (name, expected_status) in [
            ("claude_session_start_hook.py", 19),
            ("agent_session_start_hook.py", 19),
            ("claude_stop_hook.py", 23),
            ("agent_turn_complete_hook.py", 23),
        ] {
            let shim = root.join(".claude/brain-hooks").join(name);
            let status = Command::new("python3")
                .arg(&shim)
                .status()
                .expect("run compatibility shim");
            assert!(
                status.code() == Some(expected_status),
                "{shim:?} did not forward to the workspace hook in {}",
                root.display()
            );
        }
    }
}

#[test]
fn explicit_down_migration_restores_the_previous_frontend_lifecycle() {
    let fixture = Fixture::new();
    let up = fixture.run(&["server", "status"]);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.70.0",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    assert_eq!(
        configured_commands(&fixture.home.join(".codex/hooks.json"), "SessionStart"),
        vec![
            r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py""#
        ]
    );
    assert_eq!(
        configured_commands(&fixture.home.join(".codex/hooks.json"), "Stop"),
        vec![
            r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py""#
        ]
    );
    for root in [&fixture.family, &fixture.work] {
        assert!(
            !root
                .join(".brain/hooks/agent_session_start_hook.py")
                .exists()
        );
        assert!(
            !root
                .join(".brain/hooks/agent_session_stop_hook.py")
                .exists()
        );
        assert!(!root.join(".codex/hooks.json").exists());
        let old_scripts = root.join(".claude/brain-hooks");
        for name in [
            "agent_session_start_hook.py",
            "agent_turn_complete_hook.py",
            "claude_session_start_hook.py",
            "claude_stop_hook.py",
        ] {
            assert!(old_scripts.join(name).is_file(), "missing restored {name}");
        }
        assert_eq!(
            configured_commands(&root.join(".claude/settings.json"), "SessionStart"),
            vec![
                r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_session_start_hook.py""#
            ]
        );
        assert_eq!(
            configured_commands(&root.join(".claude/settings.json"), "Stop"),
            vec![
                r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_turn_complete_hook.py""#
            ]
        );
        let plugin = std::fs::read_to_string(root.join(".opencode/plugins/brain.js"))
            .expect("restored OpenCode plugin");
        assert!(plugin.contains(".claude/brain-hooks"));
        assert!(plugin.contains("agent_turn_complete_hook.py"));
        assert!(!plugin.contains("agent_session_stop_hook.py"));
    }
}

#[test]
fn task_two_down_migration_removes_only_receiver_observation_producers() {
    let fixture = Fixture::new();
    assert!(fixture.run(&["server", "status"]).status.success());
    for root in [&fixture.family, &fixture.work] {
        for settings in [
            root.join(".claude/settings.json"),
            root.join(".codex/hooks.json"),
        ] {
            let mut value: Value =
                serde_json::from_slice(&std::fs::read(&settings).expect("read lifecycle settings"))
                    .expect("parse lifecycle settings");
            for event in ["UserPromptSubmit", "PostToolUse"] {
                value["hooks"][event]
                    .as_array_mut()
                    .expect("observation event")
                    .push(json!({"hooks": [{
                        "type": "command",
                        "command": "python3 /opt/user/receiver_observation_bridge.py"
                    }]}));
            }
            std::fs::write(&settings, serde_json::to_vec_pretty(&value).unwrap())
                .expect("seed same-basename user command");
        }
    }

    let down = fixture.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.80.2",
    ]);

    assert!(
        down.status.success(),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    for root in [&fixture.family, &fixture.work] {
        assert!(
            root.join(".brain/hooks/agent_session_start_hook.py")
                .is_file()
        );
        assert!(
            root.join(".brain/hooks/agent_session_stop_hook.py")
                .is_file()
        );
        assert!(
            !root
                .join(".brain/hooks/receiver_observation_bridge.py")
                .exists()
        );
        for settings in [
            root.join(".claude/settings.json"),
            root.join(".codex/hooks.json"),
        ] {
            assert!(configured_commands(&settings, "SessionStart").len() == 1);
            assert!(configured_commands(&settings, "Stop").len() == 1);
            assert_eq!(
                configured_commands(&settings, "UserPromptSubmit"),
                vec!["python3 /opt/user/receiver_observation_bridge.py"]
            );
            assert_eq!(
                configured_commands(&settings, "PostToolUse"),
                vec!["python3 /opt/user/receiver_observation_bridge.py"]
            );
        }
        let plugin = std::fs::read_to_string(root.join(".opencode/plugins/brain.js"))
            .expect("downgraded OpenCode plugin");
        assert!(!plugin.contains("BRAIN_RECEIVER_JOB_TOKEN"));
        assert!(!plugin.contains("receiver_observation_bridge.py"));
    }
}

#[test]
fn help_and_version_do_not_run_migrations() {
    for arguments in [["--help"].as_slice(), ["--version"].as_slice()] {
        let fixture = Fixture::new();
        let global = fixture.home.join(".codex/hooks.json");
        std::fs::create_dir_all(global.parent().expect("Codex config parent"))
            .expect("Codex config");
        std::fs::write(
            &global,
            serde_json::to_vec_pretty(&json!({
                "hooks": {
                    "Stop": [{
                        "hooks": [{
                            "type": "command",
                            "command": "python3 /old/agent_turn_complete_hook.py"
                        }]
                    }]
                }
            }))
            .expect("Codex settings"),
        )
        .expect("write Codex settings");
        let before = std::fs::read(&global).expect("global settings before command");

        let output = fixture.run(arguments);

        assert!(output.status.success());
        assert_eq!(
            std::fs::read(&global).expect("global settings after command"),
            before
        );
        assert!(!fixture.xdg_config.join("brain/migrations/version").exists());
        assert!(!fixture.family.join(".brain").exists());
        assert!(!fixture.work.join(".brain").exists());
    }
}

#[test]
fn ordinary_startup_recreates_missing_workspace_artifacts_after_migration() {
    let fixture = Fixture::new();
    let first = fixture.run(&["server", "status"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let missing = fixture.work.join(".brain/hooks/agent_session_stop_hook.py");
    std::fs::remove_file(&missing).expect("remove managed hook");

    let second = fixture.run(&["server", "status"]);

    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(missing.is_file());
}

mod receiver_model {
    use super::*;

    include!("startup_migration/receiver_model.rs");
}

mod receiver_recovery_cleanup {
    use super::*;

    include!("startup_migration/receiver_recovery_cleanup.rs");
}

mod receiver_recovery_cleanup_safety {
    use super::*;

    include!("startup_migration/receiver_recovery_cleanup_safety.rs");
}

mod receiver_notice_cutover_review {
    use super::*;

    include!("startup_migration/receiver_notice_cutover_review.rs");
}

mod job_socket_cutover {
    use super::*;

    include!("startup_migration/job_socket_cutover.rs");
}
