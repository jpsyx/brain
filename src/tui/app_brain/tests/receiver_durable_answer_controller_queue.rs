use super::receiver_durable_answer_cleanup::job_state;
use super::receiver_durable_support::{accept_email_job, publish_valid_completion};
use super::*;

use crate::state::ReceiverJobState;

#[derive(Clone)]
struct ShutdownFailureRecording(Arc<Mutex<ShutdownFailureState>>);

struct ShutdownFailureState {
    failures_remaining: usize,
    shutdowns: usize,
    alive: bool,
}

impl ShutdownFailureRecording {
    fn new(failures_remaining: usize) -> Self {
        Self(Arc::new(Mutex::new(ShutdownFailureState {
            failures_remaining,
            shutdowns: 0,
            alive: false,
        })))
    }

    fn transport(&self) -> Box<dyn AgentTransport> {
        Box::new(ShutdownFailureTransport(self.clone()))
    }

    fn shutdowns(&self) -> usize {
        self.0.lock().expect("shutdown state").shutdowns
    }

    fn is_alive(&self) -> bool {
        self.0.lock().expect("shutdown state").alive
    }

    fn allow_shutdown(&self) {
        self.0.lock().expect("shutdown state").failures_remaining = 0;
    }
}

struct ShutdownFailureTransport(ShutdownFailureRecording);

impl AgentTransport for ShutdownFailureTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        self.0.0.lock().expect("shutdown state").alive = true;
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        self.0.is_alive()
    }

    fn shutdown(&mut self) -> Result<(), AgentError> {
        let mut state = self.0.0.lock().expect("shutdown state");
        state.shutdowns += 1;
        if state.failures_remaining > 0 {
            state.failures_remaining -= 1;
            return Err(AgentError::Transport(
                "injected repeatable shutdown failure".to_owned(),
            ));
        }
        state.alive = false;
        drop(state);
        Ok(())
    }
}

#[test]
fn failing_answer_shutdown_does_not_block_a_later_answer_or_exact_cleanup() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job(&app, &db, "first answer", 100);
    let second = accept_email_job(&app, &db, "second answer", 200);
    let first_shutdown = ShutdownFailureRecording::new(4);
    app.brain
        .replace_receiver_transport(first_shutdown.transport());
    app.tick_receiver();
    let first_artifact = publish_valid_completion(&app, "first durable answer");

    app.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert!(first_shutdown.is_alive());
    assert_eq!(first_shutdown.shutdowns(), 1);
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 1,
        "pending controller cleanup count was wrong"
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(first_artifact.exists());

    let second_shutdown = ShutdownFailureRecording::new(1);
    app.brain
        .replace_receiver_transport(second_shutdown.transport());
    app.tick_receiver();

    assert_eq!(first_shutdown.shutdowns(), 2);
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
    let second_artifact = publish_valid_completion(&app, "second durable answer");

    app.tick_receiver();

    assert_eq!(
        job_state(&db, second.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(first_shutdown.shutdowns(), 4);
    assert!(first_shutdown.is_alive());
    assert!(first_artifact.exists());
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 2,
        "pending controller cleanup count was wrong"
    );
    assert!(second_shutdown.is_alive());
    assert_eq!(second_shutdown.shutdowns(), 0);

    app.tick_receiver();

    assert_eq!(second_shutdown.shutdowns(), 1);
    assert!(second_shutdown.is_alive());
    assert_eq!(first_shutdown.shutdowns(), 4);
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 2,
        "pending controller cleanup count was wrong"
    );
    assert!(first_artifact.exists());
    assert!(second_artifact.exists());

    app.tick_receiver();

    assert_eq!(first_shutdown.shutdowns(), 5);
    assert!(!first_shutdown.is_alive());
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 1,
        "pending controller cleanup count was wrong"
    );
    assert!(!first_artifact.exists());

    app.tick_receiver();

    assert_eq!(second_shutdown.shutdowns(), 2);
    assert!(!second_shutdown.is_alive());
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 0,
        "pending controller cleanup count was wrong"
    );
    assert!(!second_artifact.exists());
}

#[test]
fn orderly_runtime_shutdown_retries_each_parked_answer_controller_once() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "answer before orderly shutdown", 100);
    let shutdown = ShutdownFailureRecording::new(1);
    app.brain.replace_receiver_transport(shutdown.transport());
    app.tick_receiver();
    let artifact = publish_valid_completion(&app, "durable before orderly shutdown");
    app.tick_receiver();
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 1,
        "pending controller cleanup count was wrong"
    );
    assert!(shutdown.is_alive());

    app.shutdown_receiver_runtime();

    assert_eq!(shutdown.shutdowns(), 2);
    assert!(!shutdown.is_alive());
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 0,
        "pending controller cleanup count was wrong"
    );
    assert!(!artifact.exists());
    assert_eq!(
        job_state(&db, accepted.job_id()),
        ReceiverJobState::AnswerReady
    );
}

#[test]
fn full_cleanup_registry_preserves_the_ninth_exact_controller_and_artifact() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let jobs = (0..9)
        .map(|index| accept_email_job(&app, &db, &format!("bounded answer {index}"), 100 + index))
        .collect::<Vec<_>>();
    let mut shutdowns = Vec::new();
    let mut artifacts = Vec::new();

    for (index, accepted) in jobs.iter().enumerate() {
        let shutdown = ShutdownFailureRecording::new(usize::MAX);
        app.brain.replace_receiver_transport(shutdown.transport());
        app.tick_receiver();
        assert_eq!(
            app.brain.receiver_run_observations()[0].job_id,
            accepted.job_id()
        );
        artifacts.push(publish_valid_completion(
            &app,
            &format!("durable bounded answer {index}"),
        ));
        app.tick_receiver();
        assert_eq!(
            job_state(&db, accepted.job_id()),
            ReceiverJobState::AnswerReady
        );
        shutdowns.push(shutdown);
    }

    assert!(
        app.receiver.pending_answer_controller_cleanups() == 8,
        "bounded controller cleanup count was wrong"
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        jobs[8].job_id()
    );
    assert!(artifacts.iter().all(|artifact| artifact.exists()));

    app.shutdown_receiver_runtime();

    assert!(shutdowns.iter().all(ShutdownFailureRecording::is_alive));
    assert!(artifacts.iter().all(|artifact| artifact.exists()));
    assert!(
        app.receiver.pending_answer_controller_cleanups() == 8,
        "bounded controller cleanup count changed"
    );
}

#[test]
fn orderly_shutdown_attempts_the_tabbed_controller_when_a_cleanup_slot_opens() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let jobs = (0..9)
        .map(|index| {
            accept_email_job(
                &app,
                &db,
                &format!("orderly bounded answer {index}"),
                200 + index,
            )
        })
        .collect::<Vec<_>>();
    let mut shutdowns = Vec::new();
    let mut artifacts = Vec::new();

    for (index, accepted) in jobs.iter().enumerate() {
        let shutdown = ShutdownFailureRecording::new(usize::MAX);
        app.brain.replace_receiver_transport(shutdown.transport());
        app.tick_receiver();
        assert_eq!(
            app.brain.receiver_run_observations()[0].job_id,
            accepted.job_id()
        );
        artifacts.push(publish_valid_completion(
            &app,
            &format!("orderly durable bounded answer {index}"),
        ));
        app.tick_receiver();
        shutdowns.push(shutdown);
    }
    let ninth_attempts = shutdowns[8].shutdowns();
    shutdowns[0].allow_shutdown();

    app.shutdown_receiver_runtime();

    assert!(!shutdowns[0].is_alive());
    assert_eq!(shutdowns[8].shutdowns(), ninth_attempts + 1);
    assert!(shutdowns[8].is_alive());
    assert!(artifacts[8].exists());
}
