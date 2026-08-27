use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::state::{
    ReceiverAttemptKind, ReceiverJobState, ReceiverNonterminalObservationPhase, ReceiverObservation,
};

#[derive(Clone, Default)]
struct NoticeHandoffRecording(Arc<Mutex<Vec<(InboundJob, String)>>>);

impl crate::tui::state::ReceiverNoticeDelivery for NoticeHandoffRecording {
    fn queue(
        &self,
        _command: &CommandContext,
        inbound: &InboundJob,
        message: &str,
    ) -> anyhow::Result<bool> {
        self.0
            .lock()
            .expect("notice handoff recording")
            .push((inbound.clone(), message.to_owned()));
        Ok(true)
    }
}

#[test]
fn enabled_tick_reconciles_before_restart_scan_and_cleans_the_exact_stale_run() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "stale before acceptance", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();

    let instance = app
        .receiver
        .active_durable_run()
        .expect("launched receiver")
        .attribution
        .instance()
        .to_owned();
    let exact = receiver_instance_paths(&app, &instance);
    let unrelated = receiver_instance_paths(&app, "unrelated-instance");
    for path in exact.iter().chain(&unrelated) {
        std::fs::create_dir_all(path.parent().expect("artifact parent"))
            .expect("artifact directory");
        std::fs::write(path, "private local artifact").expect("seed artifact");
    }

    let scan_db = Db::open(app.context.workspace()).expect("restart-scan DB");
    let job_id = accepted.job_id();
    let observed_shutdowns = transport.clone();
    app.receiver
        .install_after_restart_scan_hook(Box::new(move || {
            assert_eq!(
                scan_db.receiver_job(job_id).unwrap().unwrap().state(),
                ReceiverJobState::Retrying,
                "reconciliation must precede restart controls"
            );
            assert_eq!(
                observed_shutdowns.shutdowns(),
                1,
                "the stale controller must be shut down before restart controls"
            );
        }));
    clock.advance(std::time::Duration::from_secs(90));

    app.tick_receiver();

    let retried = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(retried.state(), ReceiverJobState::Retrying);
    assert_eq!(retried.retry_count(), 1);
    assert!(app.brain.receiver_run_observations().is_empty());
    for path in exact {
        assert!(!path.exists(), "exact stale artifact remains: {path:?}");
    }
    for path in unrelated {
        assert!(path.exists(), "unrelated artifact was removed: {path:?}");
    }
}

#[test]
fn due_recovery_launches_before_later_ordinary_work_in_the_persisted_frontend() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let registry_store = app.context.command().registry_store.clone();
    let mut registry = RegistryStore::load_from(registry_store.path()).expect("machine registry");
    registry
        .workspaces
        .get_mut(app.context.workspace().name())
        .expect("selected workspace record")
        .env
        .insert(
            "codex_cmd".to_owned(),
            serde_json::Value::String("codex-recovery-command".to_owned()),
        );
    registry_store
        .replace(&registry)
        .expect("persist recovery frontend command");
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let stalled = accept_email_job(&app, &db, "private message must not replay", 100);
    let stale_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(stale_transport.transport());
    app.tick_receiver();

    let active = app
        .receiver
        .active_durable_run()
        .expect("launched ordinary receiver");
    let token = active.claim.job().token();
    let owner = active.claim.claim().owner().to_owned();
    let stale_instance = active.attribution.instance().to_owned();
    let native_session_id = uuid::Uuid::new_v4().to_string();
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![&native_session_id, &stale_instance],
        )
        .expect("simulate native Codex session rotation");
    assert!(
        db.replace_receiver_binding_from_instance(&active.attribution, clock.unix_ms())
            .expect("persist exact rotated Codex binding")
    );
    for (phase, revision) in [
        (ReceiverNonterminalObservationPhase::Accepted, 1),
        (ReceiverNonterminalObservationPhase::Progressing, 2),
    ] {
        assert!(
            db.apply_receiver_observation(
                stalled.job_id(),
                &owner,
                &ReceiverObservation {
                    token,
                    instance: stale_instance.clone(),
                    session_id: native_session_id.clone(),
                    phase,
                    revision,
                    observed_at_unix_ms: clock.unix_ms(),
                    authorized_at_unix_ms: clock.unix_ms(),
                },
            )
            .expect("persist ordinary receiver progress")
        );
    }
    let later = accept_email_job(&app, &db, "later ordinary work", 200);
    let sessions_dir = temporary.path().join("codex-recovery-sessions");
    let rollout = CodexRecoveryRollout::create(&sessions_dir, &native_session_id);
    let sessions_override = crate::agent::override_codex_sessions_dir_for_test(&sessions_dir);
    let recovery_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(recovery_transport.transport());
    clock.advance(std::time::Duration::from_secs(5 * 60));

    app.tick_receiver();

    let recovered = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert_eq!(recovered.state(), ReceiverJobState::Launched);
    assert_eq!(recovered.attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(recovered.recovery_count(), 1);
    assert_eq!(
        db.receiver_job(later.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued,
        "due recovery must retain FIFO priority over later ordinary work"
    );
    let recovery = app
        .receiver
        .active_durable_run()
        .expect("same-tick recovery launch");
    assert_eq!(recovery.claim.job().id(), stalled.job_id());
    assert_ne!(recovery.attribution.instance(), stale_instance);
    assert_eq!(
        recovery.attribution.registered_session().as_str(),
        native_session_id
    );
    let specifications = recovery_transport.launch_specs();
    assert_eq!(specifications.len(), 1);
    assert!(specifications[0].command.contains("codex-recovery-command"));
    assert!(specifications[0].command.contains("resume"));
    assert_eq!(stale_transport.shutdowns(), 1);

    recovery_transport.set_alive(false);
    app.tick_receiver();

    let exited = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert_eq!(exited.state(), ReceiverJobState::Failed);
    assert_eq!(exited.last_error(), Some("recovery-launch-shutdown"));
    assert!(exited.pending_unavailable_notice());
    assert!(app.brain.receiver_run_observations().is_empty());

    drop(sessions_override);
    rollout.close();
}

#[test]
fn pending_unavailable_notice_is_handed_off_and_acked_before_fifo_advances() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services.replace_receiver_sync_runtime(Box::new(clock));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let terminal = accept_email_job(&app, &db, "private failed instruction", 100);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("terminal fixture connection")
        .execute(
            "UPDATE receiver_jobs
             SET state = 'failed', attempt_kind = 'recovery', recovery_count = 1,
                 pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [terminal.job_id().to_string()],
        )
        .expect("persist terminal unavailable intent");
    let later = accept_email_job(&app, &db, "later ordinary work", 200);
    let recording = NoticeHandoffRecording::default();
    app.services
        .replace_receiver_notice_delivery(Box::new(recording.clone()));
    let scan_db = Db::open(app.context.workspace()).expect("restart scan DB");
    let terminal_job = terminal.job_id();
    let scan_recording = recording.clone();
    app.receiver
        .install_after_restart_scan_hook(Box::new(move || {
            assert!(
                !scan_db
                    .receiver_job(terminal_job)
                    .unwrap()
                    .unwrap()
                    .pending_unavailable_notice(),
                "local handoff acknowledgement must precede restart controls"
            );
            assert_eq!(scan_recording.0.lock().unwrap().len(), 1);
        }));
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());

    app.tick_receiver();

    let handed_off = recording.0.lock().unwrap();
    assert_eq!(handed_off.len(), 1);
    assert_eq!(handed_off[0].0.authenticated_sender, "member@example.test");
    assert_eq!(
        handed_off[0].1,
        crate::server::receiver::unavailable_message()
    );
    drop(handed_off);
    assert_eq!(
        db.receiver_job(later.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched,
        "terminal notice intent must not block later FIFO work"
    );
}

fn receiver_instance_paths(app: &App, instance: &str) -> [PathBuf; 3] {
    let response = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{instance}.json"));
    let observation = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    let lock = observation.with_extension("json.lock");
    [response, observation, lock]
}

struct CodexRecoveryRollout {
    path: PathBuf,
    sessions_dir: PathBuf,
}

impl CodexRecoveryRollout {
    fn create(sessions_dir: &Path, session_id: &str) -> Self {
        let rollout_dir = sessions_dir.join("9999/12/31");
        std::fs::create_dir_all(&rollout_dir).expect("create Codex recovery rollout directory");
        let path = rollout_dir.join(format!("rollout-9999-12-31T00-00-00-{session_id}.jsonl"));
        std::fs::write(&path, "{}\n").expect("write Codex recovery rollout");
        Self {
            path,
            sessions_dir: sessions_dir.to_path_buf(),
        }
    }

    fn close(self) {
        std::fs::remove_file(self.path).expect("remove Codex recovery rollout");
        std::fs::remove_dir_all(self.sessions_dir).expect("remove Codex recovery sessions");
    }
}
