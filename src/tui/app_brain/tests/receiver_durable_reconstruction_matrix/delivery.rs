use super::super::receiver_durable_answer_cleanup::job_state;
use super::super::receiver_durable_delivery::ScriptedDeliveryExecution;
use super::super::receiver_durable_support::{
    ReceiverClock, accept_email_job, publish_valid_completion,
};
use super::super::receiver_recovery_authority::DueRecoveryFixture;
use super::*;

use crate::state::ReceiverJobState;
use crate::tui::receiver::ReceiverCleanupBoundary;

pub(super) fn assert_reconstructs_and_advances(phase: RestartPhase) {
    match phase {
        RestartPhase::CleanupGated => assert_cleanup_gated_reconstruction(),
        RestartPhase::AnswerReady => assert_answer_ready_reconstruction(),
        RestartPhase::Delivering => assert_delivering_reconstruction(),
        RestartPhase::Retrying => assert_retrying_reconstruction(),
        RestartPhase::Acknowledged => assert_acknowledged_reconstruction(),
        _ => unreachable!("delivery reconstruction received an agent phase"),
    }
}

fn assert_cleanup_gated_reconstruction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let first = fixture.accepted;
    let later = accept_email_job(&fixture.app, &fixture.db, "matrix later", 200);
    fixture
        .app
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    fixture
        .app
        .services
        .inject_receiver_recovery_commit_visible_error();
    fixture.app.tick_receiver();
    let deadline = fixture
        .db
        .receiver_job(first.job_id())
        .expect("load spawned recovery")
        .expect("spawned recovery")
        .acceptance_expires_at_unix_ms()
        .expect("recovery acceptance deadline");
    fixture.clock.advance(std::time::Duration::from_millis(
        deadline.saturating_sub(fixture.clock.unix_ms()),
    ));
    fixture.app.tick_receiver();
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);
    fixture.app.tick_receiver();
    assert_eq!(
        delivery_state(&fixture.app, first.job_id()),
        "cleanup-gated"
    );
    let cleanup_instance = fixture
        .db
        .receiver_job(first.job_id())
        .expect("load cleanup-gated job")
        .expect("cleanup-gated job")
        .recovery_cleanup_instance()
        .expect("cleanup-gated recovery instance")
        .to_owned();
    rusqlite::Connection::open(fixture.app.context.state_db_path())
        .expect("dead cleanup owner connection")
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999 WHERE brain_instance_id = ?1",
            [&cleanup_instance],
        )
        .expect("mark cleanup owner dead");
    let clock = fixture.clock.clone();
    drop(fixture);

    let (mut restarted, reopened) = reconstructed_delivery_app(&temporary, &clock);
    drive_delivery_until_follower(&mut restarted, &reopened, &clock, later.job_id());

    assert_ne!(delivery_state(&restarted, first.job_id()), "cleanup-gated");
    assert_follower_advanced(&reopened, later.job_id(), RestartPhase::CleanupGated);
}

fn assert_answer_ready_reconstruction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let clock = ReceiverClock::at_unix_ms(40_000);
    let (mut origin, db, first) = active_answer(&temporary, &clock);
    publish_valid_completion(&origin, "matrix answer-ready response");
    origin.tick_receiver();
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::AnswerReady,
        "answer-ready fixture did not reach its durable phase"
    );
    let later = accept_email_job(&origin, &db, "matrix later", 200);
    drop(db);
    drop(origin);

    let (mut restarted, reopened) = reconstructed_delivery_app(&temporary, &clock);
    drive_delivery_until_follower(&mut restarted, &reopened, &clock, later.job_id());
    restarted.tick_receiver();
    restarted.tick_receiver();

    assert!(
        job_state(&reopened, first.job_id()) == ReceiverJobState::Done,
        "fresh App did not finish the answer-ready job"
    );
    assert!(
        delivery_state(&restarted, first.job_id()) == "acknowledged",
        "fresh App did not durably acknowledge the answer-ready response"
    );
    assert_follower_advanced(&reopened, later.job_id(), RestartPhase::AnswerReady);
}

fn assert_delivering_reconstruction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let clock = ReceiverClock::at_unix_ms(50_000);
    let (mut origin, db, first) = active_answer(&temporary, &clock);
    publish_valid_completion(&origin, "matrix delivering response");
    origin.tick_receiver();
    let claim = db
        .claim_next_receiver_delivery("departed-delivery", 50_000, 50_001)
        .expect("claim delivery")
        .expect("ready response delivery");
    assert!(
        db.mark_receiver_delivery_io_started(&claim, 50_000)
            .expect("mark provider IO")
    );
    let later = accept_email_job(&origin, &db, "matrix later", 200);
    clock.advance(std::time::Duration::from_secs(2));
    drop(db);
    drop(origin);

    let (mut restarted, reopened) = reconstructed_delivery_app(&temporary, &clock);
    drive_delivery_until_follower(&mut restarted, &reopened, &clock, later.job_id());

    assert!(
        job_state(&reopened, first.job_id()) == ReceiverJobState::Retrying,
        "fresh App did not reconcile the departed delivery worker"
    );
    assert_follower_advanced(&reopened, later.job_id(), RestartPhase::Delivering);
}

fn assert_retrying_reconstruction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let clock = ReceiverClock::at_unix_ms(60_000);
    let (mut origin, db, first) = active_answer(&temporary, &clock);
    publish_valid_completion(&origin, "matrix retrying response");
    origin.tick_receiver();
    let connection = rusqlite::Connection::open(origin.context.state_db_path())
        .expect("retrying fixture connection");
    connection
        .execute(
            "UPDATE receiver_deliveries
             SET state = 'retrying', attempt_count = 1,
                 retry_at_unix_ms = 60000, first_attempt_at_unix_ms = 50000,
                 error_category = 'transport-unavailable'
             WHERE job_id = ?1",
            [first.job_id().to_string()],
        )
        .expect("seed retrying response");
    connection
        .execute(
            "UPDATE receiver_jobs SET state = 'retrying' WHERE job_id = ?1",
            [first.job_id().to_string()],
        )
        .expect("seed retrying job");
    let later = accept_email_job(&origin, &db, "matrix later", 200);
    drop(connection);
    drop(db);
    drop(origin);

    let (mut restarted, reopened) = reconstructed_delivery_app(&temporary, &clock);
    drive_delivery_until_follower(&mut restarted, &reopened, &clock, later.job_id());
    restarted.tick_receiver();

    assert!(
        job_state(&reopened, first.job_id()) == ReceiverJobState::Done,
        "fresh App did not finish the durable provider retry"
    );
    assert_follower_advanced(&reopened, later.job_id(), RestartPhase::Retrying);
}

fn assert_acknowledged_reconstruction() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let clock = ReceiverClock::at_unix_ms(70_000);
    let (mut origin, db, first) = active_answer(&temporary, &clock);
    publish_valid_completion(&origin, "matrix acknowledged response");
    origin.tick_receiver();
    origin
        .services
        .replace_receiver_delivery_execution(Box::new(ScriptedDeliveryExecution::acknowledged()));
    origin.tick_receiver();
    origin.tick_receiver();
    assert!(
        job_state(&db, first.job_id()) == ReceiverJobState::Done,
        "acknowledged fixture did not finish its source job"
    );
    assert!(
        delivery_state(&origin, first.job_id()) == "acknowledged",
        "acknowledged fixture did not persist provider authority"
    );
    let later = accept_email_job(&origin, &db, "matrix later", 200);
    drop(db);
    drop(origin);

    let (mut restarted, reopened) = reconstructed_delivery_app(&temporary, &clock);
    restarted.tick_receiver();

    assert!(
        job_state(&reopened, first.job_id()) == ReceiverJobState::Done,
        "fresh App replayed an acknowledged source job"
    );
    assert!(
        delivery_state(&restarted, first.job_id()) == "acknowledged",
        "fresh App changed durable provider acknowledgement"
    );
    assert_follower_advanced(&reopened, later.job_id(), RestartPhase::Acknowledged);
}

fn active_answer(
    temporary: &tempfile::TempDir,
    clock: &ReceiverClock,
) -> (App, Db, crate::state::ReceiverAcceptance) {
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("origin state DB");
    let first = accept_email_job(&app, &db, "matrix first", 100);
    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();
    (app, db, first)
}

fn reconstructed_delivery_app(temporary: &tempfile::TempDir, clock: &ReceiverClock) -> (App, Db) {
    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    restarted
        .services
        .replace_receiver_delivery_execution(Box::new(ScriptedDeliveryExecution::acknowledged()));
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    let db = Db::open(restarted.context.workspace()).expect("reconstructed state DB");
    (restarted, db)
}

fn drive_delivery_until_follower(
    restarted: &mut App,
    db: &Db,
    clock: &ReceiverClock,
    later: crate::state::ReceiverJobId,
) {
    for _ in 0..6 {
        restarted.tick_receiver();
        if job_state(db, later) != ReceiverJobState::Queued {
            return;
        }
        clock.advance(std::time::Duration::from_secs(61));
    }
}

fn assert_follower_advanced(db: &Db, later: crate::state::ReceiverJobId, phase: RestartPhase) {
    assert_ne!(
        job_state(db, later),
        ReceiverJobState::Queued,
        "{phase:?} reconstruction left FIFO waiting on departed runtime state"
    );
}

fn delivery_state(app: &App, job_id: crate::state::ReceiverJobId) -> String {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("delivery-state connection")
        .query_row(
            "SELECT state FROM receiver_deliveries WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("durable delivery state")
}
