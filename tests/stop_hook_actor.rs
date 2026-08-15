use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WORKSPACE_ID: &str = "11111111-1111-4111-8111-111111111111";

#[path = "stop_hook_actor/contracts.rs"]
mod contracts;

fn hook_script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name)
}

fn register_session(state_db: &Path, agent_kind: &str, session_id: &str, instance: &str) {
    rusqlite::Connection::open(state_db)
        .unwrap()
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES (?1, ?2, ?3, 42, 'test', ?4, 'member', 'sms', 1, 1)",
            rusqlite::params![agent_kind, session_id, instance, WORKSPACE_ID],
        )
        .unwrap();
}

fn attributed_hook_command(
    script: &str,
    state_db: &Path,
    response_dir: &Path,
    agent_kind: &str,
    instance: &str,
    response_id: &str,
) -> Command {
    let mut command = Command::new("python3");
    command.arg(hook_script(script));
    attributed_command(
        command,
        state_db,
        response_dir,
        agent_kind,
        instance,
        response_id,
    )
}

fn attributed_command(
    mut command: Command,
    state_db: &Path,
    response_dir: &Path,
    agent_kind: &str,
    instance: &str,
    response_id: &str,
) -> Command {
    command
        .env("BRAIN_RESPONSE_DIR", response_dir)
        .env("BRAIN_RESPONSE_ID", response_id)
        .env("BRAIN_STATE_DB", state_db)
        .env("BRAIN_WORKSPACE_ID", WORKSPACE_ID)
        .env("BRAIN_WORKSPACE", "family")
        .env("BRAIN_ROOT", "/tmp/family")
        .env("BRAIN_AGENT_KIND", agent_kind)
        .env("BRAIN_INSTANCE_ID", instance)
        .env("BRAIN_PID", "42")
        .env("BRAIN_ACTOR_ID", "member")
        .env("BRAIN_CHANNEL", "sms");
    command
}

fn spawn_hook(mut command: Command, input: &serde_json::Value) -> Child {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    drop(child.stdin.take());
    child
}

fn run_hook(command: Command, input: &serde_json::Value) -> std::process::Output {
    spawn_hook(command, input).wait_with_output().unwrap()
}

fn completion_status(state_db: &Path, session_id: &str) -> String {
    rusqlite::Connection::open(state_db)
        .unwrap()
        .query_row(
            "SELECT completion_status FROM brain_sessions WHERE agent_session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn codex_completion_uses_the_job_response_id_and_preserves_actor_context() {
    let temp = tempfile::tempdir().unwrap();
    let response_dir = temp.path().join("responses");
    let state_db = temp.path().join("state.db");
    drop(brain::state::Db::open_path(&state_db).unwrap());
    let connection = rusqlite::Connection::open(&state_db).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('codex', 'codex-thread-9', 'shell-1', 42, 'test',
                     '11111111-1111-4111-8111-111111111111', 'member', 'sms', 1, 1)",
            [],
        )
        .unwrap();
    let command = attributed_hook_command(
        "agent_session_stop_hook.py",
        &state_db,
        &response_dir,
        "codex",
        "shell-1",
        "job-7",
    );
    let output = run_hook(
        command,
        &serde_json::json!({
            "thread_id": "codex-thread-9",
            "last_assistant_message": "Done"
        }),
    );
    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(response_dir.join("job-7.json")).unwrap()).unwrap();
    assert_eq!(value["session_id"], "codex-thread-9");
    assert_eq!(value["response_id"], "job-7");
    assert_eq!(value["frontend"], "codex");
    assert_eq!(
        value["workspace_id"],
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(value["actor_id"], "member");
    assert_eq!(value["channel"], "sms");
    assert_eq!(value["completion_status"], "completed");
    let status: String = connection
        .query_row(
            "SELECT completion_status FROM brain_sessions
             WHERE agent_kind = 'codex' AND agent_session_id = 'codex-thread-9'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "completed");
}

#[test]
fn stop_completion_rechecks_the_locked_lineage_after_session_start_rotation() {
    const PAUSE_AFTER_PAYLOAD: &str = r#"
import importlib.util
import os
from pathlib import Path
import sys
import time

spec = importlib.util.spec_from_file_location("brain_stop_hook", os.environ["BRAIN_TEST_HOOK"])
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)
paused = False

def trace(frame, event, arg):
    global paused
    if not paused and event == "line" and frame.f_code is hook.main.__code__ and "message" in frame.f_locals and frame.f_locals.get("temporary") is None:
        paused = True
        Path(os.environ["BRAIN_TEST_READY"]).touch()
        while not Path(os.environ["BRAIN_TEST_GO"]).exists():
            time.sleep(0.005)
    return trace

sys.settrace(trace)
hook.main()
"#;

    let temporary = tempfile::tempdir().unwrap();
    let response_dir = temporary.path().join("responses");
    let state_db = temporary.path().join("state.db");
    drop(brain::state::Db::open_path(&state_db).unwrap());
    register_session(&state_db, "claude", "session-before-new", "shell-1");

    let ready = temporary.path().join("stop-payload-ready");
    let go = temporary.path().join("continue-stop");
    let mut stop = attributed_command(
        Command::new("python3"),
        &state_db,
        &response_dir,
        "claude",
        "shell-1",
        "job-before-new",
    );
    stop.arg("-c").arg(PAUSE_AFTER_PAYLOAD);
    stop.env("BRAIN_TEST_HOOK", hook_script("agent_session_stop_hook.py"));
    stop.env("BRAIN_TEST_READY", &ready);
    stop.env("BRAIN_TEST_GO", &go);
    let stopped = spawn_hook(
        stop,
        &serde_json::json!({
            "session_id": "session-before-new",
            "last_assistant_message": "stale response"
        }),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        ready.exists(),
        "Stop hook never reached the payload barrier"
    );

    let start = attributed_hook_command(
        "agent_session_start_hook.py",
        &state_db,
        &response_dir,
        "claude",
        "shell-1",
        "unused",
    );
    let rotated = run_hook(
        start,
        &serde_json::json!({
            "session_id": "session-after-new",
            "source": "startup",
            "hook_event_name": "SessionStart"
        }),
    );
    assert!(rotated.status.success(), "rotation failed: {rotated:?}");

    std::fs::write(&go, b"continue").unwrap();
    let completed = stopped.wait_with_output().unwrap();
    assert!(
        completed.status.success(),
        "Stop hook failed: {completed:?}"
    );

    let connection = rusqlite::Connection::open(&state_db).unwrap();
    let before: (Option<i64>, String) = connection
        .query_row(
            "SELECT locked_pid, completion_status FROM brain_sessions
             WHERE agent_session_id = 'session-before-new'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let after: (Option<i64>, String) = connection
        .query_row(
            "SELECT locked_pid, completion_status FROM brain_sessions
             WHERE agent_session_id = 'session-after-new'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(before, (None, "active".to_owned()));
    assert_eq!(after, (Some(42), "active".to_owned()));
    assert!(!response_dir.join("job-before-new.json").exists());
}

#[test]
fn artifact_publication_failure_never_commits_completion_or_leaves_a_staged_file() {
    for agent_kind in ["claude", "codex", "opencode"] {
        let temporary = tempfile::tempdir().unwrap();
        let response_dir = temporary.path().join("responses");
        let state_db = temporary.path().join("state.db");
        drop(brain::state::Db::open_path(&state_db).unwrap());
        let session_id = format!("{agent_kind}-publication-failure");
        let instance = format!("{agent_kind}-shell");
        register_session(&state_db, agent_kind, &session_id, &instance);
        std::fs::create_dir_all(response_dir.join("job-failure.json")).unwrap();

        let command = attributed_hook_command(
            "agent_session_stop_hook.py",
            &state_db,
            &response_dir,
            agent_kind,
            &instance,
            "job-failure",
        );
        let output = run_hook(
            command,
            &serde_json::json!({
                "session_id": session_id,
                "last_assistant_message": "must not complete"
            }),
        );

        assert!(output.status.success(), "hook leaked failure: {output:?}");
        assert_eq!(completion_status(&state_db, &session_id), "active");
        let entries = std::fs::read_dir(&response_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [std::ffi::OsString::from("job-failure.json")]);
    }
}
