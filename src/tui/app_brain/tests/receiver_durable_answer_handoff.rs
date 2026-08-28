use std::cell::RefCell;
use std::rc::Rc;

use super::receiver_durable_answer_cleanup::{CompletionSyncRuntime, answer_fixture, job_state};
use super::receiver_durable_support::publish_valid_completion;
use super::receiver_sync::configure_receiver_sync;
use super::*;
use rusqlite::OptionalExtension as _;

use crate::state::ReceiverJobState;

#[test]
fn another_app_cannot_take_answer_cleanup_before_origin_shutdown_handoff() {
    let (temporary, mut origin, db, first, _second, transport) = answer_fixture();
    let artifact = publish_valid_completion(&origin, "answer guarded by shutdown handoff");
    let cli = Cli::parse_from(["tasks"]);
    let other = Rc::new(RefCell::new(test_app_with_instance(
        &temporary,
        &cli,
        AgentKind::Claude,
        "other-shell-under-test",
    )));
    let other_during_commit = Rc::clone(&other);
    let artifact_during_commit = artifact.clone();
    let state_path = origin.context.state_db_path().to_path_buf();
    origin
        .receiver
        .install_after_completion_commit_hook(Box::new(move || {
            other_during_commit.borrow_mut().tick_receiver();

            assert!(
                artifact_during_commit.exists(),
                "a second App must not remove artifacts before exact controller shutdown"
            );
            assert_eq!(registration_count(&state_path), 1);
            assert_eq!(cleanup_progress(&state_path), Some((0, 0, 0)));
        }));

    origin.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(transport.shutdowns(), 1);
    assert!(!artifact.exists());
    assert_eq!(registration_count(origin.context.state_db_path()), 0);
    assert_eq!(cleanup_progress(origin.context.state_db_path()), None);
}

#[test]
fn reaped_origin_takeover_finishes_every_post_answer_effect() {
    let (temporary, mut origin, db, first, second, transport) = answer_fixture();
    let artifact = publish_valid_completion(&origin, "answer recovered after origin death");
    origin
        .receiver
        .install_after_completion_commit_hook(Box::new(|| {
            panic!("injected origin death after answer commit");
        }));

    let crash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        origin.tick_receiver();
    }));
    assert!(crash.is_err());
    assert_eq!(transport.shutdowns(), 0);
    assert!(artifact.exists());
    drop(origin);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    Db::open(restarted.context.workspace())
        .expect("open state for dead-origin reconciliation")
        .with_pid_alive(|_| false)
        .reap_dead_locks()
        .expect("reap exact dead-origin lock");
    assert_eq!(
        cleanup_progress_at(restarted.context.state_db_path()),
        Some((1, 0, 0))
    );
    restarted.receiver.record_intent(true);
    configure_receiver_sync(&restarted);
    let sync = CompletionSyncRuntime::new(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(sync.clone()));
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    std::fs::write(
        restarted.context.tasks_csv_path(),
        "task_uuid,task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assigned_to,system_key,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n\
         8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,T1,Reloaded by takeover,,not_started,p2,,false,,pablo,,,,,,,,,0,2026-08-24,,,\n",
    )
    .expect("replace task fixture");

    restarted.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert!(!artifact.exists());
    assert_eq!(cleanup_progress_at(restarted.context.state_db_path()), None);
    assert_eq!(sync.pushes(), 1);
    assert!(restarted.tasks.contains_task_named("Reloaded by takeover"));
    assert_eq!(
        restarted.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
}

fn registration_count(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .expect("open receiver state for registration count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations",
            [],
            |row| row.get(0),
        )
        .expect("remaining receiver registrations")
}

fn cleanup_progress(path: &std::path::Path) -> Option<(i64, i64, i64)> {
    cleanup_progress_at(path)
}

fn cleanup_progress_at(path: &std::path::Path) -> Option<(i64, i64, i64)> {
    rusqlite::Connection::open(path)
        .expect("open receiver state for cleanup handoff")
        .query_row(
            "SELECT controller_shutdown_acknowledged, session_released, artifacts_removed
             FROM receiver_answer_cleanups",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .expect("load cleanup handoff progress")
}
