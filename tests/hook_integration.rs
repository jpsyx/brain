//! Integration tests for `scripts/claude_session_start_hook.py`.
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
#[path = "hook_integration/installer.rs"]
mod installer;

/// Locate the hook script relative to the Cargo manifest.
fn hook_script() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("scripts")
        .join("claude_session_start_hook.py")
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
    run_hook_command(cmd, input)
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

fn start_input(session_id: &str) -> String {
    serde_json::json!({
        "session_id": session_id,
        "source": "startup",
        "hook_event_name": "SessionStart",
    })
    .to_string()
}

#[test]
fn hook_without_instance_env_is_noop() {
    let (_tmp, db) = fresh_db();
    let out = run_hook(&db, None, &start_input("claude-xyz"));
    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    assert!(
        read_session(&db, "claude-xyz").is_none(),
        "ambient claude run must not record a session"
    );
}

#[test]
fn hook_rejects_an_unregistered_workspace_session_tuple() {
    let (_tmp, db) = fresh_db();

    let out = run_hook(
        &db,
        Some(("unregistered-shell", 4242)),
        &start_input("unregistered-session"),
    );

    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    assert!(
        read_session(&db, "unregistered-session").is_none(),
        "hook events cannot create an unregistered workspace/session tuple"
    );
}

#[test]
fn hook_records_session_locked_to_instance_and_pid() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "claude-abc", "inst-1", 4242);
    let out = run_hook(&db, Some(("inst-1", 4242)), &start_input("claude-abc"));
    assert!(
        out.status.success(),
        "hook failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let row = read_session(&db, "claude-abc").expect("session recorded");
    assert_eq!(row.0, "inst-1");
    assert_eq!(row.1, Some(4242));
    assert_eq!(row.2, "claude");
    assert_eq!(row.3, "11111111-1111-4111-8111-111111111111");
    assert_eq!(row.4, "pablo");
    assert_eq!(row.5, "interactive");
}

#[test]
fn hook_without_complete_workspace_identity_is_noop() {
    let (_tmp, db) = fresh_db();
    let mut cmd = Command::new("python3");
    cmd.arg(hook_script());
    cmd.env("BRAIN_WORKSPACE_ID", "11111111-1111-4111-8111-111111111111");
    cmd.env("BRAIN_WORKSPACE", "family");
    cmd.env("BRAIN_ROOT", "/tmp/family");
    cmd.env_remove("BRAIN_ACTOR_ID");
    cmd.env("BRAIN_CHANNEL", "interactive");
    cmd.env("BRAIN_AGENT_KIND", "claude");
    cmd.env("BRAIN_INSTANCE_ID", "inst-1");
    cmd.env("BRAIN_PID", "4242");
    cmd.env("BRAIN_STATE_DB", &db);

    let out = run_hook_command(cmd, &start_input("claude-incomplete"));

    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    assert!(
        read_session(&db, "claude-incomplete").is_none(),
        "a hook without the complete selected-workspace identity must not write"
    );
}

#[test]
fn new_rotation_frees_the_prior_session_for_the_same_instance() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-A", "inst-1", 4242);
    // First session for the instance.
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-A"));
    // `/new` rotates to a fresh session id; the hook fires again.
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-B"));

    let a = read_session(&db, "sess-A").expect("A still present");
    let b = read_session(&db, "sess-B").expect("B recorded");
    assert_eq!(a.1, None, "the prior session is unlocked (resumable later)");
    assert_eq!(b.1, Some(4242), "the current session stays locked");
}

#[test]
fn re_firing_the_same_session_keeps_it_locked() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-A", "inst-1", 4242);
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-A"));
    // Resume / compact fires SessionStart again for the same id.
    run_hook(&db, Some(("inst-1", 4242)), &start_input("sess-A"));
    let a = read_session(&db, "sess-A").expect("A present");
    assert_eq!(a.1, Some(4242), "still locked to this instance");
}

#[test]
fn hook_with_malformed_stdin_is_noop_not_error() {
    let (_tmp, db) = fresh_db();
    let out = run_hook(&db, Some(("inst-1", 4242)), "not even json{");
    assert!(out.status.success(), "hook must not error on bad stdin");
}

#[test]
fn distinct_instances_get_distinct_locked_sessions() {
    // Two tasks shells each record their own session; neither frees the
    // other's (the /new free pass is scoped to the firing instance).
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-1", "inst-1", 10);
    register_session(&db, "claude", "pablo", "sess-2", "inst-2", 20);
    run_hook(&db, Some(("inst-1", 10)), &start_input("sess-1"));
    run_hook(&db, Some(("inst-2", 20)), &start_input("sess-2"));
    assert_eq!(read_session(&db, "sess-1").unwrap().1, Some(10));
    assert_eq!(read_session(&db, "sess-2").unwrap().1, Some(20));
}

#[test]
fn rotation_cannot_steal_a_session_registered_to_another_live_lineage() {
    let (_tmp, db) = fresh_db();
    register_session(&db, "claude", "pablo", "sess-1", "inst-1", 10);
    register_session(&db, "claude", "pablo", "sess-2", "inst-2", 20);

    let out = run_hook(&db, Some(("inst-1", 10)), &start_input("sess-2"));

    assert!(out.status.success(), "hook exited non-zero: {out:?}");
    let first = read_session(&db, "sess-1").expect("first lineage preserved");
    let second = read_session(&db, "sess-2").expect("second lineage preserved");
    assert_eq!((first.0.as_str(), first.1), ("inst-1", Some(10)));
    assert_eq!((second.0.as_str(), second.1), ("inst-2", Some(20)));
}

#[test]
fn hook_preserves_equal_opaque_ids_with_conflicting_immutable_attribution() {
    let (_tmp, db) = fresh_db();
    register_session(
        &db,
        "claude",
        "pablo",
        "same-opaque-id",
        "claude-instance",
        4242,
    );
    register_session(
        &db,
        "codex",
        "partner",
        "same-opaque-id",
        "codex-instance",
        4242,
    );
    let first = run_scoped_hook(
        &db,
        "claude",
        "pablo",
        "claude-instance",
        &start_input("same-opaque-id"),
    );
    let second = run_scoped_hook(
        &db,
        "codex",
        "partner",
        "codex-instance",
        &serde_json::json!({
            "thread_id": "same-opaque-id",
            "source": "startup",
            "hook_event_name": "SessionStart"
        })
        .to_string(),
    );
    assert!(first.status.success());
    assert!(second.status.success());

    let conn = Connection::open(db).unwrap();
    let mut statement = conn
        .prepare(
            "SELECT agent_kind, actor_id FROM brain_sessions
             WHERE agent_session_id = 'same-opaque-id'
             ORDER BY agent_kind",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("claude".to_owned(), "pablo".to_owned()),
            ("codex".to_owned(), "partner".to_owned()),
        ]
    );
}
