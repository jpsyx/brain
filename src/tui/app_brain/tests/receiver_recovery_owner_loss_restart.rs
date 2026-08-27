use super::receiver_recovery_authority::DueRecoveryFixture;
use super::*;

use crate::state::{ReceiverNonterminalObservationPhase, ReceiverObservation};
use crate::tui::receiver::ReceiverCleanupBoundary;

#[test]
fn dead_exact_owner_after_active_recovery_loss_is_restart_cleanable_without_touching_unrelated_registration()
 {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    fixture
        .app
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    fixture.app.tick_receiver();

    let active = fixture
        .app
        .receiver
        .active_durable_run()
        .expect("active recovery after launch");
    let job_id = active.claim.job().id();
    let token = active.claim.job().token();
    let owner = active.claim.claim().owner().to_owned();
    let instance = active.attribution.instance().to_owned();
    let session = active.attribution.registered_session().clone();
    let scope = active.attribution.scope().clone();
    assert!(
        fixture
            .db
            .apply_receiver_observation(
                job_id,
                &owner,
                &ReceiverObservation {
                    token,
                    instance: instance.clone(),
                    session_id: session.as_str().to_owned(),
                    phase: ReceiverNonterminalObservationPhase::Accepted,
                    revision: 1,
                    observed_at_unix_ms: fixture.clock.unix_ms(),
                    authorized_at_unix_ms: fixture.clock.unix_ms(),
                },
            )
            .expect("persist accepted recovery")
    );
    rusqlite::Connection::open(fixture.app.context.state_db_path())
        .expect("deadline fixture connection")
        .execute(
            "UPDATE receiver_jobs SET absolute_work_expires_at_unix_ms = ?1
             WHERE job_id = ?2",
            rusqlite::params![fixture.clock.unix_ms() + 1, job_id.to_string()],
        )
        .expect("set exact absolute deadline");
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);
    let racing_db = Db::open(fixture.app.context.workspace()).expect("racing state DB");
    let racing_clock = fixture.clock.clone();
    fixture
        .app
        .receiver
        .install_after_restart_scan_hook(Box::new(move || {
            racing_clock.advance(std::time::Duration::from_millis(1));
            racing_db
                .reconcile_next_receiver_job(racing_clock.unix_ms())
                .expect("racing exact-deadline reconciliation")
                .expect("racing cleanup effect");
        }));

    fixture.app.tick_receiver();

    assert_eq!(fixture.app.brain.receiver_run_observations().len(), 1);
    let cleanup_effect = fixture
        .db
        .reconcile_next_receiver_job(fixture.clock.unix_ms())
        .expect("reload pending cleanup")
        .expect("pending cleanup effect");
    assert!(
        !fixture
            .db
            .receiver_cleanup_registration_is_stale(&cleanup_effect)
            .expect("live owner is not stale")
    );

    let unrelated_session =
        AgentSession::new("unrelated-owner-loss-session").expect("unrelated session");
    SessionStore::register(
        &fixture.db,
        &unrelated_session,
        "unrelated-owner-loss-instance",
        i32::try_from(std::process::id()).expect("test PID"),
        &scope,
    )
    .expect("register unrelated session");
    let state_path = fixture.app.context.state_db_path().to_path_buf();
    rusqlite::Connection::open(&state_path)
        .expect("dead-owner fixture connection")
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999
             WHERE brain_instance_id = ?1",
            [&instance],
        )
        .expect("mark exact active owner dead");
    let clock = fixture.clock.clone();
    drop(fixture);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::OpenCode);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock));
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    restarted.tick_receiver();

    let reopened = Db::open(restarted.context.workspace()).expect("reopened state DB");
    let cleaned = reopened
        .receiver_job(job_id)
        .expect("reload cleaned recovery")
        .expect("cleaned recovery");
    assert!(cleaned.recovery_cleanup_instance().is_none());
    assert!(cleaned.recovery_cleanup_session_id().is_none());
    assert!(
        SessionStore::claim(&reopened, &session, "post-restart-owner", 79, &scope)
            .expect("claim exact session after dead-owner cleanup")
    );
    SessionStore::release(&reopened, "post-restart-owner").expect("release exact session");
    assert!(
        !SessionStore::claim(
            &reopened,
            &unrelated_session,
            "unrelated-competitor",
            80,
            &scope,
        )
        .expect("unrelated registration remains locked"),
        "exact dead-owner cleanup must preserve unrelated registrations"
    );
    SessionStore::release(&reopened, "unrelated-owner-loss-instance")
        .expect("release unrelated session");
}
