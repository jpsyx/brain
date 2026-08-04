use std::time::{Duration, Instant};

use super::*;

#[test]
fn concurrent_rotations_cannot_overwrite_an_authorized_target() {
    const PAUSE_AFTER_TARGET_READ: &str = r#"
import importlib.util
import os
from pathlib import Path
import sys
import time

spec = importlib.util.spec_from_file_location("brain_session_hook", os.environ["BRAIN_TEST_HOOK"])
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)
paused = False

def trace(frame, event, arg):
    global paused
    if not paused and event == "line" and frame.f_code is hook.main.__code__ and "target" in frame.f_locals:
        paused = True
        Path(os.environ["BRAIN_TEST_READY"]).touch()
        while not Path(os.environ["BRAIN_TEST_GO"]).exists():
            time.sleep(0.005)
    return trace

sys.settrace(trace)
hook.main()
"#;

    let (temporary, db) = fresh_db();
    register_session(&db, "claude", "pablo", "source-1", "inst-1", 10);
    register_session(&db, "claude", "pablo", "source-2", "inst-2", 20);
    register_session(&db, "claude", "pablo", "target", "former-owner", 30);
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE brain_sessions SET locked_pid = NULL WHERE agent_session_id = 'target'",
            [],
        )
        .unwrap();

    let ready = temporary.path().join("first-authorized");
    let go = temporary.path().join("release-first");
    let mut first_cmd = attributed_hook_command(&db, "claude", "pablo", "inst-1", 10);
    first_cmd.arg("-c").arg(PAUSE_AFTER_TARGET_READ);
    first_cmd.env("BRAIN_TEST_HOOK", hook_script());
    first_cmd.env("BRAIN_TEST_READY", &ready);
    first_cmd.env("BRAIN_TEST_GO", &go);
    let first = spawn_hook_command(first_cmd, &start_input("target"));

    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < ready_deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        ready.exists(),
        "first hook did not reach its authorized target read"
    );

    let mut second_cmd = attributed_hook_command(&db, "claude", "pablo", "inst-2", 20);
    second_cmd.arg(hook_script());
    let second = spawn_hook_command(second_cmd, &start_input("target"));

    let claim_deadline = Instant::now() + Duration::from_secs(1);
    let mut second_claimed_before_release = false;
    while Instant::now() < claim_deadline {
        second_claimed_before_release =
            read_session(&db, "target").is_some_and(|row| row.0 == "inst-2" && row.1 == Some(20));
        if second_claimed_before_release {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(&go, b"go").expect("release first hook");

    let first_output = first.wait_with_output().expect("wait first hook");
    let second_output = second.wait_with_output().expect("wait second hook");
    assert!(
        first_output.status.success(),
        "first hook failed: {first_output:?}"
    );
    assert!(
        second_output.status.success(),
        "second hook failed: {second_output:?}"
    );

    let source_1 = read_session(&db, "source-1").expect("first source preserved");
    let source_2 = read_session(&db, "source-2").expect("second source preserved");
    let target = read_session(&db, "target").expect("target claimed once");
    assert_eq!((source_1.0.as_str(), source_1.1), ("inst-1", None));
    assert_eq!(
        (source_2.0.as_str(), source_2.1),
        ("inst-2", Some(20)),
        "losing lineage was unlocked after stale authorization; second claimed before release: {second_claimed_before_release}"
    );
    assert_eq!((target.0.as_str(), target.1), ("inst-1", Some(10)));
}

#[test]
fn codex_thread_rotation_frees_only_its_prior_session() {
    let (_temporary, db) = fresh_db();
    register_session(&db, "codex", "partner", "codex-old", "codex-instance", 42);

    let output = run_scoped_hook(
        &db,
        "codex",
        "partner",
        "codex-instance",
        &serde_json::json!({
            "thread_id": "codex-new",
            "source": "startup",
            "hook_event_name": "SessionStart"
        })
        .to_string(),
    );

    assert!(output.status.success(), "hook failed: {output:?}");
    let old = read_session(&db, "codex-old").expect("prior Codex session preserved");
    let new = read_session(&db, "codex-new").expect("new Codex session recorded");
    assert_eq!(old.1, None);
    assert_eq!(
        (new.0.as_str(), new.1, new.2.as_str(), new.4.as_str()),
        ("codex-instance", Some(4242), "codex", "partner")
    );
}

#[test]
fn failed_rotation_rolls_back_and_can_be_retried() {
    let (_temporary, db) = fresh_db();
    register_session(&db, "claude", "pablo", "source", "inst-1", 10);
    let connection = Connection::open(&db).expect("open state database");
    connection
        .execute_batch(
            "CREATE TRIGGER abort_source_release
             BEFORE UPDATE OF locked_pid ON brain_sessions
             WHEN OLD.agent_session_id = 'source' AND NEW.locked_pid IS NULL
             BEGIN
               SELECT RAISE(ABORT, 'forced source release failure');
             END;",
        )
        .expect("install failure trigger");

    let failed = run_hook(&db, Some(("inst-1", 10)), &start_input("target"));

    assert!(
        failed.status.success(),
        "hook must contain the failure: {failed:?}"
    );
    assert_eq!(
        read_session(&db, "source").map(|row| (row.0, row.1)),
        Some(("inst-1".to_owned(), Some(10)))
    );
    assert!(read_session(&db, "target").is_none());

    connection
        .execute("DROP TRIGGER abort_source_release", [])
        .expect("remove failure trigger");
    let retried = run_hook(&db, Some(("inst-1", 10)), &start_input("target"));

    assert!(retried.status.success(), "retry failed: {retried:?}");
    assert_eq!(read_session(&db, "source").unwrap().1, None);
    assert_eq!(
        read_session(&db, "target").map(|row| (row.0, row.1)),
        Some(("inst-1".to_owned(), Some(10)))
    );
}
