use super::receiver_durable_support::{ReceiverClock, accept_email_job, publish_valid_completion};
use super::*;

use crate::state::{
    ReceiverAttemptKind, ReceiverJobState, ReceiverNonterminalObservationPhase, ReceiverObservation,
};

#[test]
fn restarted_tui_proves_stale_cleanup_then_resumes_the_exact_persisted_frontend() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut first = test_app(&temporary, &cli, AgentKind::Codex);
    first.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    first
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(first.context.workspace()).expect("state DB");
    let stalled = accept_email_job(&first, &db, "private restart instruction", 100);
    first
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    first.tick_receiver();

    let active = first
        .receiver
        .active_durable_run()
        .expect("launched ordinary receiver");
    let token = active.claim.job().token();
    let owner = active.claim.claim().owner().to_owned();
    let stale_instance = active.attribution.instance().to_owned();
    let native_session_id = uuid::Uuid::new_v4().to_string();
    let state_path = first.context.state_db_path().to_path_buf();
    rusqlite::Connection::open(&state_path)
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
    let mut replay_trap = db
        .receiver_job(stalled.job_id())
        .expect("load accepted replay trap")
        .expect("accepted replay trap")
        .inbound()
        .clone();
    replay_trap.prompt = "/new".to_owned();
    rusqlite::Connection::open(&state_path)
        .expect("replay-trap fixture connection")
        .execute(
            "UPDATE receiver_jobs SET inbound_json = ?1 WHERE job_id = ?2",
            rusqlite::params![
                serde_json::to_string(&replay_trap).expect("serialize replay trap"),
                stalled.job_id().to_string(),
            ],
        )
        .expect("persist accepted replay trap");
    let later = accept_email_job(&first, &db, "later ordinary work", 200);
    let sessions_dir = temporary.path().join("codex-restart-sessions");
    let rollout_dir = sessions_dir.join("9999/12/31");
    std::fs::create_dir_all(&rollout_dir).expect("create Codex rollout directory");
    let rollout = rollout_dir.join(format!(
        "rollout-9999-12-31T00-00-00-{native_session_id}.jsonl"
    ));
    std::fs::write(&rollout, "{}\n").expect("write Codex rollout");
    let sessions_override = crate::agent::override_codex_sessions_dir_for_test(&sessions_dir);
    let exact_artifacts = receiver_paths(&first, &stale_instance);
    for path in &exact_artifacts {
        std::fs::create_dir_all(path.parent().expect("artifact parent"))
            .expect("artifact directory");
        std::fs::write(path, "private local artifact").expect("seed artifact");
    }

    clock.advance(std::time::Duration::from_secs(5 * 60));
    let effect = db
        .reconcile_next_receiver_job(clock.unix_ms())
        .expect("persist restart cleanup fence")
        .expect("restart cleanup effect");
    assert_eq!(effect.cleanup_instance(), Some(stale_instance.as_str()));
    rusqlite::Connection::open(&state_path)
        .expect("stale process fixture connection")
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999 WHERE brain_instance_id = ?1",
            [&stale_instance],
        )
        .expect("mark exact registration process stale");
    drop(first);

    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock));
    let recovery_transport = TransportRecording::default();
    restarted
        .brain
        .replace_receiver_transport(recovery_transport.transport());
    let failing_state_path = state_path.clone();
    restarted.receiver.install_launch_boundary_hook(
        crate::tui::receiver::ReceiverLaunchBoundary::RecoveryPreLaunchAuthorization,
        Box::new(move || {
            rusqlite::Connection::open(failing_state_path)
                .expect("owner-store failure fixture connection")
                .execute_batch(
                    "ALTER TABLE receiver_jobs
                     RENAME TO receiver_jobs_owner_store_failure;",
                )
                .expect("inject exact pre-launch owner-store failure");
        }),
    );

    restarted.tick_receiver();

    assert!(
        recovery_transport.launch_specs().is_empty(),
        "the failed owner check must not spawn"
    );
    rusqlite::Connection::open(&state_path)
        .expect("owner-store recovery fixture connection")
        .execute_batch(
            "ALTER TABLE receiver_jobs_owner_store_failure
             RENAME TO receiver_jobs;",
        )
        .expect("restore receiver store after injected failure");
    restarted
        .brain
        .replace_receiver_transport(recovery_transport.transport());
    restarted.tick_receiver();

    let recovered = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert_eq!(recovered.state(), ReceiverJobState::Launched);
    assert_eq!(recovered.attempt_kind(), ReceiverAttemptKind::Recovery);
    assert_eq!(recovered.recovery_count(), 1);
    assert_eq!(
        db.receiver_job(later.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued
    );
    let resumed = restarted
        .receiver
        .active_durable_run()
        .expect("recovery launched after restart");
    assert_eq!(resumed.claim.job().id(), stalled.job_id());
    assert_eq!(
        resumed.attribution.scope().agent_kind(),
        AgentKind::Codex,
        "persisted frontend must win over the restarted TUI default"
    );
    assert_eq!(
        resumed.attribution.registered_session().as_str(),
        native_session_id
    );
    assert!(
        recovery_transport.launch_specs()[0]
            .command
            .contains("resume")
    );
    assert!(
        !recovery_transport.launch_specs()[0]
            .command
            .contains("/new"),
        "accepted recovery must not replay the original control prompt"
    );
    for path in exact_artifacts {
        assert!(!path.exists(), "stale exact artifact remains: {path:?}");
    }

    let completion_path = publish_valid_completion(&restarted, "recovered response");
    restarted.tick_receiver();

    let completed = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::Done);
    assert!(!completed.pending_unavailable_notice());
    assert!(restarted.brain.receiver_run_observations().is_empty());
    assert!(!completion_path.exists());

    drop(sessions_override);
    std::fs::remove_file(rollout).expect("remove Codex rollout");
    std::fs::remove_dir_all(sessions_dir).expect("remove Codex sessions");
}

fn receiver_paths(app: &App, instance: &str) -> [PathBuf; 3] {
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
