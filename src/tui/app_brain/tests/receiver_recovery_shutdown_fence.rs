use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::receiver_recovery_authority::DueRecoveryFixture;
use super::*;

use crate::state::ReceiverJobState;
use crate::tui::receiver::{ReceiverCleanupBoundary, ReceiverLaunchBoundary};

#[test]
fn persistent_shutdown_failure_through_launch_deadline_retains_the_exact_session_fence() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let later = accept_email_job(&fixture.app, &fixture.db, "later FIFO work", 200);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    advance_clock_after_spawn(&mut fixture.app, fixture.clock.clone());
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);

    fixture.app.tick_receiver();

    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(transport.shutdowns(), 0);
    assert!(!claim_recovery_session(
        &fixture,
        "competitor-before-deadline"
    ));
    let launch_deadline = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load launching recovery")
        .expect("launching recovery")
        .launch_expires_at_unix_ms()
        .expect("recovery launch deadline");
    fixture.clock.advance(std::time::Duration::from_millis(
        launch_deadline
            .saturating_sub(fixture.clock.unix_ms())
            .saturating_sub(1),
    ));
    fixture.app.tick_receiver();
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);
    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.accepted.job_id())
            .expect("load recovery before exact deadline")
            .expect("recovery before exact deadline")
            .state(),
        ReceiverJobState::Launching
    );

    fixture.clock.advance(std::time::Duration::from_millis(1));
    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 0);
    assert!(
        fixture
            .app
            .receiver
            .spawned_recovery_durable_run()
            .is_some()
    );
    assert!(
        !claim_recovery_session(&fixture, "competitor-at-deadline"),
        "reconciliation must not unlock the exact session before local shutdown"
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(later.job_id())
            .expect("load FIFO follower")
            .expect("FIFO follower")
            .state(),
        ReceiverJobState::Queued
    );

    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 1);
    assert!(claim_recovery_session(
        &fixture,
        "competitor-after-exact-ack"
    ));
    SessionStore::release(&fixture.db, "competitor-after-exact-ack")
        .expect("release post-ack competing claim");
    let cleaned = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load acknowledged cleanup")
        .expect("acknowledged cleanup");
    assert!(cleaned.recovery_cleanup_instance().is_none());
    assert!(cleaned.recovery_cleanup_session_id().is_none());
    assert!(
        fixture
            .db
            .reconcile_next_receiver_job(fixture.clock.unix_ms())
            .expect("cleanup no longer redrives")
            .is_none()
    );
    fixture.app.tick_receiver();
    assert_ne!(
        fixture
            .db
            .receiver_job(later.job_id())
            .expect("reload FIFO follower")
            .expect("FIFO follower")
            .state(),
        ReceiverJobState::Queued
    );
}

#[test]
fn visible_launch_write_attaches_exact_cleanup_even_when_local_commit_was_ambiguous() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    fixture
        .app
        .services
        .inject_receiver_recovery_commit_visible_error();

    fixture.app.tick_receiver();

    let deadline = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load ambiguous visible launch")
        .expect("ambiguous visible launch")
        .acceptance_expires_at_unix_ms()
        .expect("acceptance deadline");
    fixture.clock.advance(std::time::Duration::from_millis(
        deadline.saturating_sub(fixture.clock.unix_ms()),
    ));
    fixture.app.tick_receiver();
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);
    fixture.app.tick_receiver();

    let terminal = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load exact ambiguous cleanup")
        .expect("exact ambiguous cleanup");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(
        terminal.recovery_cleanup_instance(),
        fixture
            .app
            .receiver
            .spawned_recovery_durable_run()
            .map(|run| run.attribution.instance()),
        "an acknowledgement failure must retain local authority for the exact tuple"
    );
    assert_eq!(
        terminal.recovery_cleanup_session_id(),
        Some(fixture.session.as_str())
    );
    assert!(
        !claim_recovery_session(&fixture, "competitor-after-ambiguous-ack-failure"),
        "the exact tuple must remain locked until its acknowledgement commits"
    );

    let state_path = fixture.app.context.state_db_path().to_path_buf();
    let clock = fixture.clock.clone();
    let job_id = fixture.accepted.job_id();
    rusqlite::Connection::open(&state_path)
        .expect("restart PID fixture connection")
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999
             WHERE brain_instance_id = ?1",
            [terminal
                .recovery_cleanup_instance()
                .expect("exact cleanup instance")],
        )
        .expect("mark exact cleanup owner dead");
    drop(terminal);
    drop(fixture);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::OpenCode);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    restarted.tick_receiver();

    let reopened = Db::open(restarted.context.workspace()).expect("reopen cleanup state");
    let cleaned = reopened
        .receiver_job(job_id)
        .expect("reload restarted cleanup")
        .expect("restarted cleanup");
    assert!(cleaned.recovery_cleanup_instance().is_none());
    assert!(cleaned.recovery_cleanup_session_id().is_none());
    assert!(
        reopened
            .reconcile_next_receiver_job(clock.unix_ms())
            .expect("restarted cleanup no longer redrives")
            .is_none()
    );
}

#[test]
fn orderly_shutdown_of_ambiguous_spawn_preserves_exact_restart_cleanup_proof() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    fixture
        .app
        .services
        .inject_receiver_recovery_commit_visible_error();
    fixture.app.tick_receiver();
    let instance = fixture
        .app
        .receiver
        .spawned_recovery_durable_run()
        .expect("ambiguous spawned recovery")
        .attribution
        .instance()
        .to_owned();

    fixture.app.shutdown_receiver_runtime();

    assert_eq!(transport.shutdowns(), 1);
    let terminal = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load orderly shutdown cleanup")
        .expect("orderly shutdown cleanup");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(
        terminal.recovery_cleanup_instance(),
        Some(instance.as_str())
    );
    assert_eq!(
        terminal.recovery_cleanup_session_id(),
        Some(fixture.session.as_str())
    );
    assert!(!claim_recovery_session(
        &fixture,
        "competitor-after-orderly-shutdown"
    ));
}

#[test]
fn shutdown_after_claim_expiry_terminalizes_spawned_recovery_immediately() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let later = accept_email_job(&fixture.app, &fixture.db, "later FIFO work", 200);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    advance_clock_after_spawn(&mut fixture.app, fixture.clock.clone());

    fixture.app.tick_receiver();

    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(transport.shutdowns(), 1);
    let terminal = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load cleanup after expired claim")
        .expect("cleanup after expired claim");
    assert_eq!(
        terminal.state(),
        ReceiverJobState::AnswerReady,
        "successful shutdown must terminalize now even after the recovery lease expires"
    );
    assert!(terminal.recovery_cleanup_instance().is_none());
    assert!(terminal.recovery_cleanup_session_id().is_none());
    assert!(
        fixture
            .db
            .receiver_delivery_counts()
            .unwrap()
            .answer_ready()
            == 1,
        "expired-claim cleanup changed the answer-ready count"
    );
    fixture.app.tick_receiver();
    assert_ne!(
        fixture
            .db
            .receiver_job(later.job_id())
            .expect("load FIFO follower")
            .expect("FIFO follower")
            .state(),
        ReceiverJobState::Queued,
        "terminal cleanup must unblock FIFO without waiting for the launch deadline"
    );
}

#[test]
fn pre_spawn_reconciliation_effect_is_exactly_acknowledged_after_shutdown() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    fixture.app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::RecoveryPreLaunchAuthorization,
        Box::new({
            let clock = fixture.clock.clone();
            move || clock.advance(std::time::Duration::from_secs(31))
        }),
    );
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);

    fixture.app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    assert_eq!(transport.shutdowns(), 0);
    let deadline = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load pre-spawn recovery")
        .expect("pre-spawn recovery")
        .launch_expires_at_unix_ms()
        .expect("pre-spawn launch deadline");
    fixture.clock.advance(std::time::Duration::from_millis(
        deadline.saturating_sub(fixture.clock.unix_ms()),
    ));
    fixture.app.tick_receiver();

    let fenced = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load reconciled pre-spawn recovery")
        .expect("reconciled pre-spawn recovery");
    assert_eq!(fenced.state(), ReceiverJobState::Failed);
    assert_eq!(
        fenced.recovery_cleanup_session_id(),
        Some(fixture.session.as_str()),
        "pre-spawn registration evidence must become an exact durable cleanup tuple"
    );
    assert!(!claim_recovery_session(
        &fixture,
        "competitor-before-pre-spawn-shutdown"
    ));

    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 1);
    let terminal = fixture
        .db
        .receiver_job(fixture.accepted.job_id())
        .expect("load pre-spawn terminal cleanup")
        .expect("pre-spawn terminal cleanup");
    assert_eq!(terminal.state(), ReceiverJobState::AnswerReady);
    assert!(
        terminal.recovery_cleanup_instance().is_none(),
        "the attributed pre-spawn effect must be acknowledged, not bypassed"
    );
    assert!(terminal.recovery_cleanup_session_id().is_none());
    assert!(
        fixture
            .db
            .receiver_delivery_counts()
            .unwrap()
            .answer_ready()
            == 1,
        "pre-spawn cleanup changed the answer-ready count"
    );
}

fn advance_clock_after_spawn(app: &mut App, clock: ReceiverClock) {
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Spawn,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
}

fn claim_recovery_session(fixture: &DueRecoveryFixture, owner: &str) -> bool {
    let scope = SessionScope::new(
        AgentKind::Claude,
        fixture.app.context.workspace().id(),
        email_actor(),
    );
    SessionStore::claim(&fixture.db, &fixture.session, owner, 77, &scope)
        .expect("claim exact recovery session")
}
