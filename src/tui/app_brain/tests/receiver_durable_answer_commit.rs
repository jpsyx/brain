use super::receiver_durable_answer_cleanup::{
    answer_fixture, completion_evidence_count, delivery_count, job_state,
};
use super::receiver_durable_support::publish_valid_completion;

use crate::state::ReceiverJobState;

#[test]
fn crash_before_answer_commit_retains_agent_work_and_blocks_the_next_job() {
    let (_temporary, mut app, db, first, second, transport) = answer_fixture();
    let artifact = publish_valid_completion(&app, "answer before commit crash");
    app.receiver
        .install_after_completion_validation_hook(Box::new(|| {
            panic!("injected crash before answer commit");
        }));

    let crash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.tick_receiver()));

    assert!(crash.is_err());
    assert_eq!(job_state(&db, first.job_id()), ReceiverJobState::Launched);
    assert_eq!(job_state(&db, second.job_id()), ReceiverJobState::Queued);
    assert_eq!(delivery_count(&app, first.job_id()), 0);
    assert!(
        db.receiver_conversation(first.conversation_id())
            .expect("load pre-commit conversation")
            .expect("durable conversation")
            .transcript_markdown()
            .is_empty()
    );
    assert!(artifact.exists());
    assert_eq!(transport.shutdowns(), 0);
}

#[test]
fn crash_after_answer_commit_preserves_one_answer_and_releases_the_agent_lane() {
    let (_temporary, mut app, db, first, second, transport) = answer_fixture();
    let artifact = publish_valid_completion(&app, "answer survives post-commit crash");
    app.receiver
        .install_after_completion_commit_hook(Box::new(|| {
            panic!("injected crash after answer commit");
        }));

    let crash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.tick_receiver()));

    assert!(crash.is_err());
    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(job_state(&db, second.job_id()), ReceiverJobState::Queued);
    assert_eq!(delivery_count(&app, first.job_id()), 1);
    assert_eq!(completion_evidence_count(&app, first.job_id()), 1);
    let transcript = db
        .receiver_conversation(first.conversation_id())
        .expect("load post-commit conversation")
        .expect("durable conversation")
        .transcript_markdown()
        .to_owned();
    assert_eq!(
        transcript
            .matches("answer survives post-commit crash")
            .count(),
        1
    );
    assert!(artifact.exists(), "post-commit cleanup did not run");
    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(
        db.claim_next_receiver_run("restart-owner", 2, 30_002)
            .expect("claim after post-commit crash")
            .expect("later agent work is available")
            .job()
            .id(),
        second.job_id()
    );
}
