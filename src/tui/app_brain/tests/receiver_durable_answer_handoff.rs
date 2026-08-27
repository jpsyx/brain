use std::cell::RefCell;
use std::rc::Rc;

use super::receiver_durable_answer_cleanup::{answer_fixture, job_state};
use super::receiver_durable_support::publish_valid_completion;
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
