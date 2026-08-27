use super::receiver_durable_support::accept_email_job;
use super::receiver_recovery_authority::DueRecoveryFixture;
use super::*;

use crate::state::{ReceiverJobState, ReceiverNonterminalObservationPhase, ReceiverObservation};
use crate::tui::receiver::ReceiverCleanupBoundary;

#[test]
fn active_recovery_renewal_loss_retains_controller_until_exact_cleanup_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let later = accept_email_job(&fixture.app, &fixture.db, "later FIFO work", 200);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
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
    for _ in 0..3 {
        fixture
            .app
            .receiver
            .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);
    }
    let racing_db = Db::open(fixture.app.context.workspace()).expect("racing state DB");
    let racing_clock = fixture.clock.clone();
    let (effect_sender, effect_receiver) = std::sync::mpsc::sync_channel(1);
    fixture
        .app
        .receiver
        .install_after_restart_scan_hook(Box::new(move || {
            racing_clock.advance(std::time::Duration::from_millis(1));
            let effect = racing_db
                .reconcile_next_receiver_job(racing_clock.unix_ms())
                .expect("racing exact-deadline reconciliation")
                .expect("racing cleanup effect");
            effect_sender.send(effect).expect("record cleanup effect");
        }));

    fixture.app.tick_receiver();

    let effect = effect_receiver.recv().expect("exact cleanup effect");
    let terminal = fixture
        .db
        .receiver_job(job_id)
        .expect("load terminal recovery")
        .expect("terminal recovery");
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(effect.cleanup_instance(), Some(instance.as_str()));
    assert_eq!(effect.cleanup_session_id(), Some(session.as_str()));
    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(fixture.app.brain.receiver_run_observations().len(), 1);
    assert!(
        !SessionStore::claim(&fixture.db, &session, "competing-live-tui", 77, &scope)
            .expect("reject competing native-session claim")
    );
    assert!(
        !fixture
            .db
            .acknowledge_receiver_recovery_cleanup(
                job_id,
                token,
                &instance,
                "wrong-native-session",
                fixture.clock.unix_ms(),
            )
            .expect("reject wrong cleanup acknowledgement")
    );
    assert!(
        !fixture
            .db
            .receiver_cleanup_registration_is_stale(&effect)
            .expect("live cleanup owner is not restart proof")
    );

    fixture.app.tick_receiver();
    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(fixture.app.brain.receiver_run_observations().len(), 1);
    assert!(
        !SessionStore::claim(&fixture.db, &session, "competing-after-retries", 78, &scope)
            .expect("retain native-session lock through repeated shutdown failure")
    );

    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 1);
    assert!(
        fixture
            .app
            .brain
            .receiver_run_observations()
            .iter()
            .all(|observation| observation.instance != instance),
        "the exact recovery tab must be gone even when later FIFO work launches"
    );
    let cleaned = fixture
        .db
        .receiver_job(job_id)
        .expect("reload acknowledged cleanup")
        .expect("acknowledged cleanup");
    assert!(cleaned.recovery_cleanup_instance().is_none());
    assert!(cleaned.recovery_cleanup_session_id().is_none());
    assert!(
        SessionStore::claim(&fixture.db, &session, "competing-after-ack", 79, &scope)
            .expect("claim native session after exact acknowledgement")
    );
    SessionStore::release(&fixture.db, "competing-after-ack")
        .expect("release competing native-session claim");
    fixture.app.tick_receiver();
    assert_ne!(
        fixture
            .db
            .receiver_job(later.job_id())
            .expect("reload FIFO follower")
            .expect("FIFO follower")
            .state(),
        ReceiverJobState::Queued,
        "exact acknowledgement must let later FIFO work advance"
    );
}

#[test]
fn active_recovery_observation_cas_loss_uses_the_same_exact_cleanup_fence() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    fixture.app.tick_receiver();
    let active = fixture
        .app
        .receiver
        .active_durable_run()
        .expect("active recovery after launch");
    let job_id = active.claim.job().id();
    let instance = active.attribution.instance().to_owned();
    let session = active.attribution.registered_session().clone();
    let snapshot = fixture
        .app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    std::fs::create_dir_all(snapshot.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        &snapshot,
        serde_json::json!({
            "version": 1,
            "revision": 1,
            "phase": "accepted",
            "job_token": active.claim.job().token().to_string(),
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": null,
            "accepted_at_unix_ms": fixture.clock.unix_ms(),
            "progressing_at_unix_ms": null,
            "latest_progress_at_unix_ms": null,
            "completed_at_unix_ms": null,
        })
        .to_string(),
    )
    .expect("write accepted observation");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&snapshot, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
    rusqlite::Connection::open(fixture.app.context.state_db_path())
        .expect("deadline fixture connection")
        .execute(
            "UPDATE receiver_jobs SET acceptance_expires_at_unix_ms = ?1
             WHERE job_id = ?2",
            rusqlite::params![fixture.clock.unix_ms() + 1, job_id.to_string()],
        )
        .expect("set exact acceptance deadline");
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);
    let racing_db = Db::open(fixture.app.context.workspace()).expect("racing state DB");
    let racing_clock = fixture.clock.clone();
    let (effect_sender, effect_receiver) = std::sync::mpsc::sync_channel(1);
    fixture
        .app
        .receiver
        .install_after_observation_validation_hook(Box::new(move || {
            racing_clock.advance(std::time::Duration::from_millis(1));
            let effect = racing_db
                .reconcile_next_receiver_job(racing_clock.unix_ms())
                .expect("racing exact-deadline reconciliation")
                .expect("racing cleanup effect");
            effect_sender.send(effect).expect("record cleanup effect");
        }));

    fixture.app.tick_receiver();

    let effect = effect_receiver
        .try_recv()
        .expect("observation validation race must run");
    assert_eq!(effect.cleanup_instance(), Some(instance.as_str()));
    assert_eq!(effect.cleanup_session_id(), Some(session.as_str()));
    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(fixture.app.brain.receiver_run_observations().len(), 1);
    let terminal = fixture.db.receiver_job(job_id).unwrap().unwrap();
    assert_eq!(terminal.state(), ReceiverJobState::Failed);
    assert_eq!(
        terminal.recovery_cleanup_instance(),
        Some(instance.as_str())
    );
    assert_eq!(
        terminal.recovery_cleanup_session_id(),
        Some(session.as_str())
    );
}
