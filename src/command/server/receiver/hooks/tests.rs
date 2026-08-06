use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::json;

use super::{
    command, install_for_home, replace_entry, update_json_file, update_json_file_with_temporary,
};

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
        Path::new("/Users/member-b/fam-brain/.claude/brain-hooks/claude_stop_hook.py"),
        Path::new("/Users/member-b/fam-brain"),
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
fn concurrent_workspace_registrations_and_unrelated_settings_survive() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hooks.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "permissions": {"allow": ["Read"]}
        }))
        .unwrap(),
    )
    .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

    std::thread::scope(|scope| {
        for workspace in ["family", "work"] {
            let path = path.clone();
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                update_json_file(&path, |settings| {
                    let basename = format!("{workspace}.py");
                    let command = format!("python3 /workspaces/{workspace}/{basename}");
                    replace_entry(settings, "SessionStart", &basename, &command);
                })
                .unwrap();
            });
        }
    });

    let bytes = std::fs::read(path).unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(settings["permissions"]["allow"][0], "Read");
    let commands = settings["hooks"]["SessionStart"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["hooks"][0]["command"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        commands,
        std::collections::BTreeSet::from([
            "python3 /workspaces/family/family.py",
            "python3 /workspaces/work/work.py",
        ])
    );
}

#[test]
fn failed_atomic_hook_replacement_preserves_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hooks.json");
    let original = br#"{"permissions":{"allow":["Read"]}}"#;
    std::fs::write(&path, original).unwrap();
    let blocked_temporary = temp.path().join("blocked-temporary");
    std::fs::create_dir(&blocked_temporary).unwrap();

    let result = update_json_file_with_temporary(&path, &blocked_temporary, |settings| {
        settings["changed"] = json!(true);
    });

    assert!(result.is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn concurrent_workspace_installs_preserve_both_roots_and_shared_codex_json() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let family = temp.path().join("family");
    let work = temp.path().join("work");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(&family).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        home.join(".codex/hooks.json"),
        serde_json::to_vec(&json!({"permissions": {"allow": ["Read"]}})).unwrap(),
    )
    .unwrap();

    std::thread::scope(|scope| {
        for root in [&family, &work] {
            let home = home.clone();
            scope.spawn(move || install_for_home(root, &home).unwrap());
        }
    });

    for root in [&family, &work] {
        assert_eq!(
            configured_command(&root.join(".claude/settings.json"), "SessionStart"),
            "python3 .claude/brain-hooks/claude_session_start_hook.py"
        );
        assert_eq!(
            configured_command(&root.join(".claude/settings.json"), "Stop"),
            "python3 .claude/brain-hooks/claude_stop_hook.py"
        );
    }
    let codex_bytes = std::fs::read(home.join(".codex/hooks.json")).unwrap();
    let codex: serde_json::Value = serde_json::from_slice(&codex_bytes).unwrap();
    assert_eq!(codex["permissions"]["allow"][0], "Read");
    assert_eq!(codex["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    assert_eq!(codex["hooks"]["Stop"].as_array().unwrap().len(), 1);
}

#[test]
fn stop_hook_recovers_final_message_from_transcript_when_field_absent() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("brain");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    install_for_home(&root, &home).unwrap();
    let settings = root.join(".claude/settings.json");
    let start = configured_command(&settings, "SessionStart");
    let stop = configured_command(&settings, "Stop");

    let db_path = temp.path().join("state.db");
    drop(crate::state::Db::open_path(&db_path).unwrap());
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('claude', 'pending-claude-launch', 'instance-1', 4242, 'test-launch',
                     '11111111-1111-4111-8111-111111111111',
                     'pablo', 'sms', 1, 1)",
            [],
        )
        .unwrap();
    drop(connection);

    // A realistic Claude Code transcript: the turn ends on a text message,
    // preceded by a thinking-only assistant message. Claude Code sends
    // `last_assistant_message` today, but the hook must not depend on it — an
    // absent field is the failure mode that silently dropped every SMS reply.
    let transcript = temp.path().join("session.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"question"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"the final answer"}]}}"#,
            "\n",
        ),
    )
    .unwrap();

    let response_dir = temp.path().join("responses");
    let common = [
        (
            "BRAIN_WORKSPACE_ID",
            std::ffi::OsStr::new("11111111-1111-4111-8111-111111111111"),
        ),
        ("BRAIN_WORKSPACE", std::ffi::OsStr::new("brain")),
        ("BRAIN_ROOT", root.as_os_str()),
        ("BRAIN_ACTOR_ID", std::ffi::OsStr::new("pablo")),
        ("BRAIN_CHANNEL", std::ffi::OsStr::new("sms")),
        ("BRAIN_AGENT_KIND", std::ffi::OsStr::new("claude")),
        ("BRAIN_INSTANCE_ID", std::ffi::OsStr::new("instance-1")),
        ("BRAIN_PID", std::ffi::OsStr::new("4242")),
        ("BRAIN_STATE_DB", db_path.as_os_str()),
        ("BRAIN_RESPONSE_DIR", response_dir.as_os_str()),
        (
            "BRAIN_RESPONSE_ID",
            std::ffi::OsStr::new("response-claude-1"),
        ),
    ];

    let started = run_configured(
        &root,
        &start,
        &common,
        &json!({"session_id":"claude-session-1","source":"startup"}),
    );
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );

    let stopped = run_configured(
        &root,
        &stop,
        &common,
        &json!({
            "session_id":"claude-session-1",
            "transcript_path": transcript.to_string_lossy(),
            "hook_event_name":"Stop",
            "stop_hook_active": false
        }),
    );
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );

    let response: serde_json::Value = serde_json::from_slice(
        &std::fs::read(response_dir.join("response-claude-1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(response["message"], "the final answer");
    assert_eq!(response["session_id"], "claude-session-1");
    assert_eq!(response["channel"], "sms");
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
    let codex_schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&codex_hooks).unwrap()).unwrap();
    assert_eq!(
        codex_schema["hooks"]["SessionStart"],
        json!([{"hooks": [{
            "type": "command",
            "command": "python3 .claude/brain-hooks/claude_session_start_hook.py"
        }]}])
    );
    assert_eq!(
        codex_schema["hooks"]["Stop"],
        json!([{"hooks": [{
            "type": "command",
            "command": "python3 .claude/brain-hooks/claude_stop_hook.py"
        }]}])
    );
    let start = configured_command(&codex_hooks, "SessionStart");
    let stop = configured_command(&codex_hooks, "Stop");
    let db_path = temp.path().join("state.db");
    drop(crate::state::Db::open_path(&db_path).unwrap());
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('codex', 'pending-codex-launch', 'instance-1', 4242, 'test-launch',
                     '11111111-1111-4111-8111-111111111111',
                     'pablo', 'interactive', 1, 1)",
            [],
        )
        .unwrap();
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
            "SELECT agent_kind, actor_id, channel, completion_status FROM brain_sessions
             WHERE agent_session_id = 'codex-thread-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        attribution,
        (
            "codex".to_owned(),
            "pablo".to_owned(),
            "interactive".to_owned(),
            "completed".to_owned()
        )
    );
    let response: serde_json::Value =
        serde_json::from_slice(&std::fs::read(response_dir.join("response-1.json")).unwrap())
            .unwrap();
    assert_eq!(response["session_id"], "codex-thread-1");
    assert_eq!(response["frontend"], "codex");
    assert_eq!(
        response["workspace_id"],
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(response["actor_id"], "pablo");
    assert_eq!(response["channel"], "interactive");
}
