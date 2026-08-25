use super::receiver_durable_support::{
    ReceiverClock, accept_email_job, accept_email_job_in_thread, publish_valid_rotated_completion,
};
use super::*;

use crate::state::ReceiverJobState;
use crate::tui::receiver::ReceiverLaunchBoundary;

#[test]
fn expired_claim_during_capability_planning_failure_is_not_retried() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let mut config = app.context.config().clone();
    config.access_mode = crate::access::AccessMode::WorkspaceOnly;
    app.context = app.context.replacing_config(config);
    std::fs::write(
        app.context.workspace().root().join(".config/config.json"),
        "{",
    )
    .expect("write malformed capability config");
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow capability failure", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::CapabilityPlanning,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(transport.launch_specs().is_empty());
    assert_eq!(transport.shutdowns(), 0);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
    assert_eq!(job.last_error(), None);
}

#[test]
fn expired_claim_after_capability_planning_stops_before_the_availability_probe() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow capability success", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let availability_probes = Arc::new(Mutex::new(0));
    let observed_probes = Arc::clone(&availability_probes);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::CapabilityPlanning,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::AvailabilityProbe,
        Box::new(move || *observed_probes.lock().expect("probe count") += 1),
    );

    app.tick_receiver();

    assert_eq!(*availability_probes.lock().expect("probe count"), 0);
    assert!(transport.launch_specs().is_empty());
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
}

#[test]
fn expired_claim_after_availability_probe_stops_before_registration() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow availability probe", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let registrations = Arc::new(Mutex::new(0));
    let observed_registrations = Arc::clone(&registrations);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::AvailabilityProbe,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || {
            *observed_registrations.lock().expect("registration count") += 1;
        }),
    );

    app.tick_receiver();

    assert_eq!(*registrations.lock().expect("registration count"), 0);
    assert!(transport.launch_specs().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
}

#[test]
fn expired_claim_after_resume_validation_stops_before_session_registration() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job_in_thread(&app, &db, "slow-resume", "first message", 100);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    publish_valid_rotated_completion(&app, "session-1", "first response");
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(first.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );

    let second = accept_email_job_in_thread(&app, &db, "slow-resume", "second message", 200);
    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());
    let registrations = Arc::new(Mutex::new(0));
    let observed_registrations = Arc::clone(&registrations);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::ResumeValidation,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || {
            *observed_registrations.lock().expect("registration count") += 1;
        }),
    );

    app.tick_receiver();

    assert_eq!(*registrations.lock().expect("registration count"), 0);
    assert!(second_transport.launch_specs().is_empty());
    assert_eq!(second_transport.shutdowns(), 1);
    let job = db.receiver_job(second.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
}

#[test]
fn expired_claim_during_registration_failure_is_not_retried() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow registration failure", 100);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("registration fault connection")
        .execute_batch(
            "CREATE TRIGGER fail_receiver_registration
             BEFORE INSERT ON receiver_session_registrations
             BEGIN
               SELECT RAISE(FAIL, 'injected receiver registration failure');
             END;",
        )
        .expect("install registration failure");
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(transport.launch_specs().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
    assert_eq!(job.last_error(), None);
}

#[test]
fn expired_claim_after_successful_registration_releases_the_exact_registration() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow registration success", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );

    app.tick_receiver();

    assert!(transport.launch_specs().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
    let registrations = rusqlite::Connection::open(app.context.state_db_path())
        .expect("registration inspection connection")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("registration count");
    assert_eq!(registrations, 0);
}

#[test]
fn expired_claim_during_resume_registration_failure_does_not_fall_back_fresh() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job_in_thread(&app, &db, "resume-registration", "first", 100);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    publish_valid_rotated_completion(&app, "session-1", "first response");
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(first.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );

    let second = accept_email_job_in_thread(&app, &db, "resume-registration", "second", 200);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("registration fault connection")
        .execute_batch(
            "CREATE TRIGGER fail_resume_registration
             BEFORE INSERT ON receiver_session_registrations
             BEGIN
               SELECT RAISE(FAIL, 'injected receiver registration failure');
             END;",
        )
        .expect("install registration failure");
    let second_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(second_transport.transport());
    let fallback_registrations = Arc::new(Mutex::new(0));
    let observed_fallbacks = Arc::clone(&fallback_registrations);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Registration,
        Box::new(move || *observed_fallbacks.lock().expect("fallback count") += 1),
    );

    app.tick_receiver();

    assert_eq!(*fallback_registrations.lock().expect("fallback count"), 0);
    assert!(second_transport.launch_specs().is_empty());
    let job = db.receiver_job(second.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Claimed);
    assert_eq!(job.retry_count(), 0);
}
