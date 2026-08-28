use super::receiver_durable_support::{ReceiverClock, accept_email_job, publish_valid_completion};
use super::*;

use crate::state::{ReceiverAttemptKind, ReceiverJobState, ReceiverNonterminalObservationPhase};

#[test]
fn every_frontend_recovers_one_stalled_native_session_through_the_controller_facade() {
    for kind in AgentKind::ALL {
        assert_frontend_recovery_lifecycle(kind);
    }
}

fn assert_frontend_recovery_lifecycle(kind: AgentKind) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, kind);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let private_inbound = format!("private-{kind:?}-inbound-must-not-replay");
    let stalled = accept_email_job(&app, &db, &private_inbound, 100);
    let ordinary_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(ordinary_transport.transport());
    app.tick_receiver();

    let ordinary = app
        .receiver
        .active_durable_run()
        .expect("launched ordinary receiver");
    let ordinary_owner = ordinary.claim.claim().owner().to_owned();
    let ordinary_instance = ordinary.attribution.instance().to_owned();
    let native_session = native_session_for(kind);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("native-session fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![native_session.as_str(), &ordinary_instance],
        )
        .expect("simulate native session rotation");
    assert!(
        db.replace_receiver_binding_from_instance(&ordinary.attribution, clock.unix_ms())
            .expect("persist exact native binding")
    );
    for (phase, revision) in [
        (ReceiverNonterminalObservationPhase::Accepted, 1),
        (ReceiverNonterminalObservationPhase::Progressing, 2),
    ] {
        assert!(
            db.apply_receiver_observation(
                stalled.job_id(),
                &ordinary_owner,
                &crate::state::ReceiverObservation {
                    token: ordinary.claim.job().token(),
                    instance: ordinary_instance.clone(),
                    session_id: native_session.as_str().to_owned(),
                    phase,
                    revision,
                    observed_at_unix_ms: clock.unix_ms(),
                    authorized_at_unix_ms: clock.unix_ms(),
                },
            )
            .expect("persist ordinary lifecycle")
        );
    }

    let _claude_transcript = (kind == AgentKind::Claude)
        .then(|| ClaudeTranscript::create(app.context.workspace().root(), native_session.as_str()));
    let codex_sessions_dir = temporary.path().join("codex-recovery-matrix");
    if kind == AgentKind::Codex {
        let rollout_dir = codex_sessions_dir.join("9999/12/31");
        std::fs::create_dir_all(&rollout_dir).expect("Codex rollout directory");
        std::fs::write(
            rollout_dir.join(format!(
                "rollout-9999-12-31T00-00-00-{}.jsonl",
                native_session.as_str()
            )),
            "{}\n",
        )
        .expect("Codex rollout");
    }
    let codex_override = (kind == AgentKind::Codex)
        .then(|| crate::agent::override_codex_sessions_dir_for_test(&codex_sessions_dir));
    let recovery_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(recovery_transport.transport());
    clock.advance(std::time::Duration::from_secs(5 * 60));

    app.tick_receiver();

    let recovered = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert_eq!(recovered.state(), ReceiverJobState::Launched, "{kind:?}");
    assert!(
        recovered.attempt_kind() == ReceiverAttemptKind::Recovery,
        "frontend recovery used the wrong attempt kind"
    );
    assert_eq!(recovered.recovery_count(), 1, "{kind:?}");
    let recovery = app.receiver.active_durable_run().expect("active recovery");
    assert_ne!(
        recovery.attribution.instance(),
        ordinary_instance,
        "{kind:?} recovery needs a fresh remote instance"
    );
    assert_eq!(
        recovery.attribution.registered_session(),
        &native_session,
        "{kind:?} recovery must retain the exact native session"
    );
    assert_eq!(ordinary_transport.shutdowns(), 1, "{kind:?}");
    let specifications = recovery_transport.launch_specs();
    assert_eq!(specifications.len(), 1, "{kind:?}");
    let command = &specifications[0].command;
    match kind {
        AgentKind::Claude => assert!(command.contains("--resume"), "{command}"),
        AgentKind::Codex => assert!(command.contains("resume"), "{command}"),
        AgentKind::OpenCode => assert!(command.contains("--session"), "{command}"),
    }
    assert!(command.contains(native_session.as_str()), "{kind:?}");
    assert!(!command.contains(&private_inbound), "{kind:?}");
    assert!(
        command.contains(&recovered.token().to_string()),
        "{kind:?} recovery prompt needs the opaque token marker"
    );

    write_recovery_snapshot(
        &app,
        &native_session,
        clock.unix_ms(),
        1,
        "accepted",
        false,
        false,
    );
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(stalled.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Accepted,
        "{kind:?}"
    );
    write_recovery_snapshot(
        &app,
        &native_session,
        clock.unix_ms(),
        2,
        "progressing",
        true,
        false,
    );
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(stalled.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Processing,
        "{kind:?}"
    );
    write_recovery_snapshot(
        &app,
        &native_session,
        clock.unix_ms(),
        3,
        "completed",
        true,
        true,
    );
    let completion_path = publish_valid_completion(&app, "recorded recovered response");
    app.tick_receiver();

    let completed = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::AnswerReady, "{kind:?}");
    assert!(
        completed.observation_revision() == 3,
        "frontend recovery changed the observation revision"
    );
    assert!(!completed.pending_unavailable_notice(), "{kind:?}");
    assert!(!completion_path.exists(), "{kind:?}");
    assert!(app.brain.receiver_run_observations().is_empty(), "{kind:?}");

    drop(codex_override);
    if kind == AgentKind::Codex {
        std::fs::remove_dir_all(codex_sessions_dir).expect("remove Codex recovery matrix");
    }
}

fn native_session_for(kind: AgentKind) -> AgentSession {
    let value = match kind {
        AgentKind::Claude => format!("claude-recovery-{}", uuid::Uuid::new_v4()),
        AgentKind::Codex => uuid::Uuid::new_v4().to_string(),
        AgentKind::OpenCode => "session-1".to_owned(),
    };
    AgentSession::new(value).expect("native session")
}

fn write_recovery_snapshot(
    app: &App,
    session: &AgentSession,
    now: u64,
    revision: u64,
    phase: &str,
    progressed: bool,
    completed: bool,
) {
    let active = app.receiver.active_durable_run().expect("active recovery");
    let path = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{}.json", active.attribution.instance()));
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "revision": revision,
            "phase": phase,
            "job_token": active.claim.job().token().to_string(),
            "instance_id": active.attribution.instance(),
            "session_id": session.as_str(),
            "turn_id": progressed.then_some("recovery-turn"),
            "accepted_at_unix_ms": now,
            "progressing_at_unix_ms": progressed.then_some(now),
            "latest_progress_at_unix_ms": progressed.then_some(now),
            "completed_at_unix_ms": completed.then_some(now),
        })
        .to_string(),
    )
    .expect("recovery observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only recovery observation");
    }
}
