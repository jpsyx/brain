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
use std::process::{Command, Stdio};

use rusqlite::Connection;
use brain::state::Db;

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
    drop(Db::open(&db_path).expect("open"));
    (tmp, db_path)
}

/// Run the hook with the given attribution env (None → ambient, no env set)
/// and stdin payload.
fn run_hook(
    db_path: &Path,
    attribution: Option<(&str, i32)>,
    input: &str,
) -> std::process::Output {
    let mut cmd = Command::new("python3");
    cmd.arg(hook_script());
    cmd.env_remove("BRAIN_INSTANCE_ID");
    cmd.env_remove("BRAIN_PID");
    cmd.env("BRAIN_STATE_DB", db_path);
    if let Some((instance, pid)) = attribution {
        cmd.env("BRAIN_INSTANCE_ID", instance);
        cmd.env("BRAIN_PID", pid.to_string());
    }
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
    child.wait_with_output().expect("wait hook")
}

/// Read (instance, locked_pid) for a session row, or None if absent.
fn read_session(db_path: &Path, session_id: &str) -> Option<(String, Option<i64>)> {
    let conn = Connection::open(db_path).unwrap();
    conn.query_row(
        "SELECT brain_instance_id, locked_pid FROM brain_sessions
         WHERE claude_session_id = ?1",
        [session_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
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
fn hook_records_session_locked_to_instance_and_pid() {
    let (_tmp, db) = fresh_db();
    let out = run_hook(&db, Some(("inst-1", 4242)), &start_input("claude-abc"));
    assert!(
        out.status.success(),
        "hook failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let row = read_session(&db, "claude-abc").expect("session recorded");
    assert_eq!(row.0, "inst-1");
    assert_eq!(row.1, Some(4242));
}

#[test]
fn new_rotation_frees_the_prior_session_for_the_same_instance() {
    let (_tmp, db) = fresh_db();
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
    run_hook(&db, Some(("inst-1", 10)), &start_input("sess-1"));
    run_hook(&db, Some(("inst-2", 20)), &start_input("sess-2"));
    assert_eq!(read_session(&db, "sess-1").unwrap().1, Some(10));
    assert_eq!(read_session(&db, "sess-2").unwrap().1, Some(20));
}
