use super::receiver_durable_support::{ReceiverClock, accept_email_job};
pub(super) use super::receiver_recovery_native_cleanup_support::{
    NativeCleanupRun, conversation_binding, fixture_connection,
    launch_prior_bound_rotated_accepted_run, registration_actual_session, registration_count,
    seed_receiver_artifacts,
};
use super::receiver_recovery_native_cleanup_support::{
    claim_native_session, launch_rotated_accepted_run,
};
use super::*;

use crate::state::{ReceiverReconciliationAction, ReceiverReconciliationReason};
use crate::tui::receiver::ReceiverCleanupBoundary;

#[test]
fn accepted_stall_rotated_native_cleanup_releases_only_after_app_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let run = launch_rotated_accepted_run(&mut app, &db, &clock, "accepted stall", 100);
    let later = accept_email_job(&app, &db, "later FIFO work", 200);
    let artifacts = seed_receiver_artifacts(&app, &run.instance);
    app.receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);
    clock.advance(std::time::Duration::from_secs(5 * 60));

    app.tick_receiver();

    assert_eq!(transport.shutdowns(), 1);
    assert!(artifacts.iter().all(|path| !path.exists()));
    assert_eq!(
        registration_actual_session(&run.state_path, &run.instance).as_deref(),
        Some(run.native.as_str())
    );
    assert!(!claim_native_session(&db, &run, "competing-before-ack"));

    app.receiver.record_intent(false);
    app.tick_receiver();
    app.tick_receiver();

    assert!(claim_native_session(&db, &run, "competing-after-ack"));
    SessionStore::release(&db, "competing-after-ack").expect("release competing claim");
    let recovery = db
        .claim_next_receiver_recovery_run(
            "recovery-after-cleanup",
            clock.unix_ms(),
            clock.unix_ms() + 30_000,
        )
        .expect("claim cleanup-acknowledged recovery")
        .expect("cleanup acknowledgement unblocks recovery");
    assert_eq!(recovery.job().id(), run.job_id);
    assert_eq!(
        db.receiver_job(later.job_id()).unwrap().unwrap().state(),
        crate::state::ReceiverJobState::Queued
    );
}

#[test]
fn absolute_expiry_rotated_native_cleanup_releases_only_after_app_acknowledgement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let run = launch_rotated_accepted_run(&mut app, &db, &clock, "absolute expiry", 100);
    let later = accept_email_job(&app, &db, "later FIFO work", 200);
    fixture_connection(&run)
        .execute(
            "UPDATE receiver_jobs SET absolute_work_expires_at_unix_ms = ?1
             WHERE workspace_id = ?2 AND job_id = ?3 AND job_token = ?4",
            rusqlite::params![
                clock.unix_ms(),
                app.context.workspace().id().to_string(),
                run.job_id.to_string(),
                run.token.to_string(),
            ],
        )
        .expect("expire exact accepted run");
    let artifacts = seed_receiver_artifacts(&app, &run.instance);
    app.receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);

    app.tick_receiver();

    let terminal = db.receiver_job(run.job_id).unwrap().unwrap();
    assert_eq!(terminal.state(), crate::state::ReceiverJobState::Failed);
    assert_eq!(
        terminal.last_error(),
        Some("recovery-absolute-work-expired")
    );
    assert_eq!(transport.shutdowns(), 1);
    assert!(artifacts.iter().all(|path| !path.exists()));
    assert_eq!(
        registration_actual_session(&run.state_path, &run.instance).as_deref(),
        Some(run.native.as_str())
    );
    assert!(!claim_native_session(&db, &run, "competing-before-ack"));

    app.receiver.record_intent(false);
    app.tick_receiver();
    app.tick_receiver();

    assert!(claim_native_session(&db, &run, "competing-after-ack"));
    SessionStore::release(&db, "competing-after-ack").expect("release competing claim");
    let next = db
        .claim_next_receiver_run(
            "later-after-cleanup",
            clock.unix_ms(),
            clock.unix_ms() + 30_000,
        )
        .expect("claim later FIFO work")
        .expect("terminal cleanup acknowledgement unblocks FIFO work");
    assert_eq!(next.job().id(), later.job_id());
}

#[test]
fn restarted_tui_releases_dead_unbound_fresh_cleanup_registration() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut first = test_app(&temporary, &cli, AgentKind::Claude);
    first.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    first
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(first.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&first, &db, "fresh pre-acceptance", 100);
    first
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    first.tick_receiver();
    let active = first
        .receiver
        .active_durable_run()
        .expect("launched fresh receiver");
    let run = NativeCleanupRun {
        job_id: accepted.job_id(),
        token: active.claim.job().token(),
        conversation_id: accepted.conversation_id(),
        instance: active.attribution.instance().to_owned(),
        registered: active.attribution.registered_session().clone(),
        native: active.attribution.registered_session().clone(),
        scope: active.attribution.scope().clone(),
        state_path: first.context.state_db_path().to_path_buf(),
        workspace_id: first.context.workspace().id().to_string(),
    };
    let artifacts = seed_receiver_artifacts(&first, &run.instance);
    fixture_connection(&run)
        .execute(
            "UPDATE receiver_jobs
             SET retry_count = 2, acceptance_expires_at_unix_ms = ?1
             WHERE workspace_id = ?2 AND job_id = ?3 AND job_token = ?4",
            rusqlite::params![
                clock.unix_ms(),
                first.context.workspace().id().to_string(),
                run.job_id.to_string(),
                run.token.to_string(),
            ],
        )
        .expect("exhaust exact fresh pre-acceptance run");
    let effect = db
        .reconcile_next_receiver_job(clock.unix_ms())
        .expect("persist exact fresh cleanup fence")
        .expect("fresh cleanup effect");
    assert_eq!(
        effect.action(),
        ReceiverReconciliationAction::TerminalFailure
    );
    assert_eq!(
        effect.reason(),
        ReceiverReconciliationReason::PreAcceptanceExhausted
    );
    assert_eq!(effect.cleanup_instance(), Some(run.instance.as_str()));
    assert_eq!(effect.cleanup_session_id(), Some(run.native.as_str()));
    assert_eq!(conversation_binding(&run), (None, None));
    assert!(!claim_native_session(&db, &run, "competing-before-ack"));
    fixture_connection(&run)
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999
             WHERE workspace_id = ?1 AND brain_instance_id = ?2
               AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
               AND agent_session_id = ?6",
            rusqlite::params![
                first.context.workspace().id().to_string(),
                &run.instance,
                run.scope.agent_kind().as_str(),
                run.scope.actor().user_id().as_str(),
                run.scope.actor().channel().as_str(),
                run.native.as_str(),
            ],
        )
        .expect("mark exact fresh registration owner dead");
    drop(first);

    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock));
    restarted.tick_receiver();

    assert!(artifacts.iter().all(|path| !path.exists()));
    assert!(claim_native_session(&db, &run, "competing-after-ack"));
    let cleaned = db.receiver_job(run.job_id).unwrap().unwrap();
    assert_eq!(cleaned.recovery_cleanup_instance(), None);
    assert_eq!(cleaned.recovery_cleanup_session_id(), None);
    assert_eq!(registration_count(&run), 0);
}
