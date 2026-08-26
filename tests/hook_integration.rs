//! Integration tests for `scripts/agent_session_start_hook.py`.
//!
//! We exercise the real Python script with a controlled env and stdin (the
//! hook input JSON claude would send) against a temp sqlite DB created by the
//! real `Db::open` migration. The assertion is on the `brain_sessions` state
//! after the hook exits — did it record the session locked to the right
//! instance/pid, free a prior session on `/new`, and correctly no-op when
//! attribution metadata is missing?

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use brain::state::Db;
use rusqlite::Connection;

#[path = "hook_integration/atomic.rs"]
mod atomic;
#[path = "hook_integration/contracts.rs"]
mod contracts;
#[path = "hook_integration/installer.rs"]
mod installer;
#[path = "hook_integration/receiver_lifecycle.rs"]
mod receiver_lifecycle;

/// Locate the hook script relative to the Cargo manifest.
fn hook_script() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("scripts")
        .join("agent_session_start_hook.py")
}

/// Fresh temp DB with the real schema (via `Db::open`).
fn fresh_db() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("state.db");
    drop(Db::open_path(&db_path).expect("open"));
    (tmp, db_path)
}

fn register_session(
    db_path: &Path,
    agent_kind: &str,
    actor_id: &str,
    session_id: &str,
    instance: &str,
    pid: i32,
) {
    let connection = Connection::open(db_path).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES (?1, ?2, ?3, ?4, 'test-launch',
                     '11111111-1111-4111-8111-111111111111', ?5, 'interactive', 1, 1)",
            rusqlite::params![agent_kind, session_id, instance, pid, actor_id],
        )
        .unwrap();
}

/// Run the hook with the given attribution env (None → ambient, no env set)
/// and stdin payload.
fn run_hook(db_path: &Path, attribution: Option<(&str, i32)>, input: &str) -> std::process::Output {
    let mut cmd = Command::new("python3");
    cmd.arg(hook_script());
    for name in [
        "BRAIN_WORKSPACE_ID",
        "BRAIN_WORKSPACE",
        "BRAIN_ROOT",
        "BRAIN_ACTOR_ID",
        "BRAIN_CHANNEL",
        "BRAIN_AGENT_KIND",
        "BRAIN_INSTANCE_ID",
        "BRAIN_PID",
        "BRAIN_STATE_DB",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("BRAIN_STATE_DB", db_path);
    if let Some((instance, pid)) = attribution {
        cmd.env("BRAIN_WORKSPACE_ID", "11111111-1111-4111-8111-111111111111");
        cmd.env("BRAIN_WORKSPACE", "family");
        cmd.env("BRAIN_ROOT", "/tmp/family");
        cmd.env("BRAIN_ACTOR_ID", "pablo");
        cmd.env("BRAIN_CHANNEL", "interactive");
        cmd.env("BRAIN_AGENT_KIND", "claude");
        cmd.env("BRAIN_INSTANCE_ID", instance);
        cmd.env("BRAIN_PID", pid.to_string());
    }
    run_hook_command(cmd, input)
}

fn run_scoped_hook(
    db_path: &Path,
    agent_kind: &str,
    actor_id: &str,
    instance: &str,
    input: &str,
) -> std::process::Output {
    run_hook_command(
        scoped_hook_command(db_path, agent_kind, actor_id, instance),
        input,
    )
}

fn scoped_hook_command(
    db_path: &Path,
    agent_kind: &str,
    actor_id: &str,
    instance: &str,
) -> Command {
    let mut cmd = Command::new("python3");
    cmd.arg(hook_script());
    cmd.env("BRAIN_WORKSPACE_ID", "11111111-1111-4111-8111-111111111111");
    cmd.env("BRAIN_WORKSPACE", "family");
    cmd.env("BRAIN_ROOT", "/tmp/family");
    cmd.env("BRAIN_ACTOR_ID", actor_id);
    cmd.env("BRAIN_CHANNEL", "interactive");
    cmd.env("BRAIN_AGENT_KIND", agent_kind);
    cmd.env("BRAIN_INSTANCE_ID", instance);
    cmd.env("BRAIN_PID", "4242");
    cmd.env("BRAIN_STATE_DB", db_path);
    cmd
}

fn spawn_hook_command(mut cmd: Command, input: &str) -> Child {
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn python3");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    child
}

fn run_hook_command(cmd: Command, input: &str) -> std::process::Output {
    spawn_hook_command(cmd, input)
        .wait_with_output()
        .expect("wait hook")
}

fn attributed_hook_command(
    db_path: &Path,
    agent_kind: &str,
    actor_id: &str,
    instance: &str,
    pid: i32,
) -> Command {
    let mut cmd = Command::new("python3");
    cmd.env("BRAIN_WORKSPACE_ID", "11111111-1111-4111-8111-111111111111");
    cmd.env("BRAIN_WORKSPACE", "family");
    cmd.env("BRAIN_ROOT", "/tmp/family");
    cmd.env("BRAIN_ACTOR_ID", actor_id);
    cmd.env("BRAIN_CHANNEL", "interactive");
    cmd.env("BRAIN_AGENT_KIND", agent_kind);
    cmd.env("BRAIN_INSTANCE_ID", instance);
    cmd.env("BRAIN_PID", pid.to_string());
    cmd.env("BRAIN_STATE_DB", db_path);
    cmd
}

/// Read immutable attribution plus lock state for a session row.
fn read_session(
    db_path: &Path,
    session_id: &str,
) -> Option<(String, Option<i64>, String, String, String, String)> {
    let conn = Connection::open(db_path).unwrap();
    conn.query_row(
        "SELECT brain_instance_id, locked_pid, agent_kind, workspace_id, actor_id, channel
         FROM brain_sessions WHERE agent_session_id = ?1",
        [session_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )
    .ok()
}

/// The SessionStart payload a *forked* session sends. Background agents fork
/// the panel's conversation and inherit its `BRAIN_*` environment, so this is
/// what reaches the hook when one starts.
fn fork_input(session_id: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "source": "fork",
        "hook_event_name": "SessionStart",
    })
    .to_string()
}

fn start_input(session_id: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "source": "startup",
        "hook_event_name": "SessionStart",
    })
    .to_string()
}

#[path = "hook_integration/lifecycle.rs"]
mod lifecycle;
