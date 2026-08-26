use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::state::ReceiverJobState;
use crate::tui::receiver::ReceiverLaunchBoundary;

struct ClockAdvancingFailingSpawn {
    clock: ReceiverClock,
    shutdowns: Arc<Mutex<usize>>,
}

impl AgentTransport for ClockAdvancingFailingSpawn {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        self.clock.advance(std::time::Duration::from_secs(31));
        Err(AgentError::Transport("injected spawn failure".to_owned()))
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        false
    }

    fn shutdown(&mut self) {
        *self.shutdowns.lock().expect("shutdown count") += 1;
    }
}

struct ClockAdvancingSuccessfulSpawn {
    clock: ReceiverClock,
    spawns: Arc<Mutex<usize>>,
    shutdowns: Arc<Mutex<usize>>,
}

struct ShutdownAdvancingTransport {
    clock: ReceiverClock,
    spawn_fails: bool,
    shutdowns: Arc<Mutex<usize>>,
}

struct ShutdownAdvancingActiveTransport {
    clock: ReceiverClock,
    alive: Arc<Mutex<bool>>,
    shutdowns: Arc<Mutex<usize>>,
}

impl AgentTransport for ShutdownAdvancingActiveTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        *self.alive.lock().expect("alive state") = true;
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        *self.alive.lock().expect("alive state")
    }

    fn shutdown(&mut self) {
        self.clock.advance(std::time::Duration::from_secs(7));
        *self.shutdowns.lock().expect("shutdown count") += 1;
        *self.alive.lock().expect("alive state") = false;
    }
}

impl AgentTransport for ShutdownAdvancingTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        if self.spawn_fails {
            Err(AgentError::Transport("injected spawn failure".to_owned()))
        } else {
            Ok(())
        }
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        !self.spawn_fails
    }

    fn shutdown(&mut self) {
        self.clock.advance(std::time::Duration::from_secs(7));
        *self.shutdowns.lock().expect("shutdown count") += 1;
    }
}

impl AgentTransport for ClockAdvancingSuccessfulSpawn {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        *self.spawns.lock().expect("spawn count") += 1;
        self.clock.advance(std::time::Duration::from_secs(31));
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn shutdown(&mut self) {
        *self.shutdowns.lock().expect("shutdown count") += 1;
    }
}

#[test]
fn expired_claim_during_spawn_failure_is_cleaned_without_recording_a_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow spawn failure", 100);
    let shutdowns = Arc::new(Mutex::new(0));
    app.brain
        .replace_receiver_transport(Box::new(ClockAdvancingFailingSpawn {
            clock,
            shutdowns: Arc::clone(&shutdowns),
        }));

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Launching);
    assert_eq!(job.retry_count(), 0);
    assert_eq!(job.last_error(), None);
}

#[test]
fn expired_claim_during_successful_spawn_stops_before_tab_allocation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow successful spawn", 100);
    let spawns = Arc::new(Mutex::new(0));
    let shutdowns = Arc::new(Mutex::new(0));
    app.brain
        .replace_receiver_transport(Box::new(ClockAdvancingSuccessfulSpawn {
            clock: clock.clone(),
            spawns: Arc::clone(&spawns),
            shutdowns: Arc::clone(&shutdowns),
        }));
    let allocations = Arc::new(Mutex::new(0));
    let observed_allocations = Arc::clone(&allocations);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Allocation,
        Box::new(move || *observed_allocations.lock().expect("allocation count") += 1),
    );

    app.tick_receiver();

    assert_eq!(*spawns.lock().expect("spawn count"), 1);
    assert_eq!(*allocations.lock().expect("allocation count"), 0);
    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    assert!(app.brain.receiver_run_observations().is_empty());
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Launching);
    assert_eq!(job.retry_count(), 0);
    let registrations: i64 = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open state DB for registration count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2",
            rusqlite::params![
                app.context.workspace().id().to_string(),
                accepted.conversation_id().to_string(),
            ],
            |row| row.get(0),
        )
        .expect("count preserved session registration");
    assert_eq!(registrations, 1);
    assert!(
        db.claim_next_receiver_run("competing-owner", clock.unix_ms(), clock.unix_ms() + 30_000,)
            .expect("poll ambiguous successful spawn")
            .is_none()
    );
}

#[test]
fn expired_claim_during_tab_allocation_failure_is_not_retried() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow allocation failure", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    crate::tui::state::exhaust_session_tab_ids(&mut app.brain);
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Allocation,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(transport.shutdowns(), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Launched);
    assert_eq!(job.retry_count(), 0);
    assert_eq!(job.last_error(), None);
}

#[test]
fn expired_claim_after_successful_tab_allocation_removes_only_the_new_tab() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "slow allocation success", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Allocation,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );

    app.tick_receiver();

    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(transport.shutdowns(), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Launched);
    assert_eq!(job.retry_count(), 0);
}

#[test]
fn spawn_failure_retry_uses_the_clock_observed_after_controller_cleanup() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "cleanup before spawn retry", 100);
    let shutdowns = Arc::new(Mutex::new(0));
    app.brain
        .replace_receiver_transport(Box::new(ShutdownAdvancingTransport {
            clock: clock.clone(),
            spawn_fails: true,
            shutdowns: Arc::clone(&shutdowns),
        }));

    app.tick_receiver();

    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Retrying);
    assert_eq!(job.retry_count(), 1);
    assert_eq!(job.retry_at_unix_ms(), Some(clock.unix_ms() + 5_000));
}

#[test]
fn allocation_failure_after_successful_spawn_remains_launched_and_nonclaimable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "cleanup before allocation retry", 100);
    let shutdowns = Arc::new(Mutex::new(0));
    app.brain
        .replace_receiver_transport(Box::new(ShutdownAdvancingTransport {
            clock: clock.clone(),
            spawn_fails: false,
            shutdowns: Arc::clone(&shutdowns),
        }));
    crate::tui::state::exhaust_session_tab_ids(&mut app.brain);

    app.tick_receiver();

    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Launched);
    assert_eq!(job.retry_count(), 0);
    assert_eq!(job.retry_at_unix_ms(), None);
    clock.advance(std::time::Duration::from_secs(31));
    assert!(
        db.claim_next_receiver_run("competing-owner", clock.unix_ms(), clock.unix_ms() + 30_000,)
            .expect("poll allocation ambiguity")
            .is_none()
    );
}

#[test]
fn launch_commit_failure_after_successful_spawn_preserves_nonclaimable_correlation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "launch commit failure", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    let state_db_path = app.context.state_db_path().to_path_buf();
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Spawn,
        Box::new(move || {
            rusqlite::Connection::open(&state_db_path)
                .expect("state DB at spawn boundary")
                .execute_batch(
                    "CREATE TRIGGER fail_receiver_launch_commit
                     BEFORE UPDATE OF state ON receiver_jobs
                     WHEN NEW.state = 'launched'
                     BEGIN
                       SELECT RAISE(FAIL, 'injected launch commit failure');
                     END;",
                )
                .expect("install launch-commit failure");
        }),
    );

    app.tick_receiver();

    assert_eq!(transport.launch_specs().len(), 1);
    assert_eq!(transport.shutdowns(), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job.state(), ReceiverJobState::Launching);
    assert_eq!(job.retry_count(), 0);
    let registrations: i64 = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open state DB for registration count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2",
            rusqlite::params![
                app.context.workspace().id().to_string(),
                accepted.conversation_id().to_string(),
            ],
            |row| row.get(0),
        )
        .expect("count preserved session registration");
    assert_eq!(registrations, 1);
    clock.advance(std::time::Duration::from_secs(31));
    assert!(
        db.claim_next_receiver_run("competing-owner", clock.unix_ms(), clock.unix_ms() + 30_000,)
            .expect("poll commit ambiguity")
            .is_none()
    );
}

#[test]
fn child_exit_cleanup_does_not_mutate_launched_state_when_shutdown_advances_clock() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "cleanup before child-exit retry", 100);
    let alive = Arc::new(Mutex::new(false));
    let shutdowns = Arc::new(Mutex::new(0));
    app.brain
        .replace_receiver_transport(Box::new(ShutdownAdvancingActiveTransport {
            clock,
            alive: Arc::clone(&alive),
            shutdowns: Arc::clone(&shutdowns),
        }));
    app.tick_receiver();
    let durable_before = db
        .receiver_job(accepted.job_id())
        .expect("load launched job")
        .expect("launched job");
    *alive.lock().expect("alive state") = false;

    app.tick_receiver();

    assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
    let job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(job, durable_before);
}
