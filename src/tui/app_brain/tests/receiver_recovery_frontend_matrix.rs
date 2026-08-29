use super::receiver_durable_support::{ReceiverClock, accept_email_job, publish_valid_completion};
use super::*;

use crate::state::{ReceiverAttemptKind, ReceiverJobState, ReceiverNonterminalObservationPhase};

#[test]
fn every_frontend_reconstructs_then_recovers_through_the_controller_facade() {
    assert_reconstructed_frontend_recovery_matrix();
}

pub(super) fn assert_reconstructed_frontend_recovery_matrix() {
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
    let later = accept_email_job(&app, &db, "later reconstruction work", 200);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("dead-origin fixture connection")
        .execute(
            "UPDATE brain_sessions SET locked_pid = 999999 WHERE brain_instance_id = ?1",
            [&ordinary_instance],
        )
        .expect("mark origin process dead");
    clock.advance(std::time::Duration::from_secs(5 * 60));
    drop(app);

    let mut restarted = test_app(&temporary, &cli, reconstructed_default(kind));
    restarted.receiver.record_intent(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let recovery_transport = TransportRecording::default();
    restarted
        .brain
        .replace_receiver_transport(recovery_transport.transport());
    for _ in 0..4 {
        restarted.tick_receiver();
        if !recovery_transport.launch_specs().is_empty() {
            break;
        }
    }

    let recovered = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert!(
        recovered.state() == ReceiverJobState::Launched,
        "{kind:?} recovery recorded the wrong launched state"
    );
    assert!(
        recovered.attempt_kind() == ReceiverAttemptKind::Recovery,
        "frontend recovery used the wrong attempt kind"
    );
    assert!(
        recovered.recovery_count() == 1,
        "frontend recovery recorded the wrong recovery count for {kind:?}"
    );
    let recovery = restarted
        .receiver
        .active_durable_run()
        .expect("active reconstructed recovery");
    assert_ne!(
        recovery.attribution.instance(),
        ordinary_instance,
        "{kind:?} recovery needs a fresh isolated-run instance"
    );
    assert_eq!(
        recovery.attribution.registered_session(),
        &native_session,
        "{kind:?} recovery must retain the exact native session"
    );
    assert_eq!(
        ordinary_transport.shutdowns(),
        0,
        "{kind:?} reconstruction depended on the departed controller"
    );
    let specifications = recovery_transport.launch_specs();
    assert_eq!(specifications.len(), 1, "{kind:?}");
    let command = &specifications[0].command;
    match kind {
        AgentKind::Claude => assert!(command.contains("--resume"), "{kind:?}"),
        AgentKind::Codex => assert!(command.contains("resume"), "{kind:?}"),
        AgentKind::OpenCode => assert!(command.contains("--session"), "{kind:?}"),
    }
    assert!(command.contains(native_session.as_str()), "{kind:?}");
    assert!(!command.contains(&private_inbound), "{kind:?}");
    assert!(
        command.contains(&recovered.token().to_string()),
        "{kind:?} recovery prompt needs the opaque token marker"
    );

    write_recovery_snapshot(
        &restarted,
        &native_session,
        clock.unix_ms(),
        1,
        "accepted",
        false,
        false,
    );
    restarted.tick_receiver();
    assert!(
        db.receiver_job(stalled.job_id()).unwrap().unwrap().state() == ReceiverJobState::Accepted,
        "{kind:?} recovery recorded the wrong accepted state"
    );
    write_recovery_snapshot(
        &restarted,
        &native_session,
        clock.unix_ms(),
        2,
        "progressing",
        true,
        false,
    );
    restarted.tick_receiver();
    assert!(
        db.receiver_job(stalled.job_id()).unwrap().unwrap().state() == ReceiverJobState::Processing,
        "{kind:?} recovery recorded the wrong processing state"
    );
    write_recovery_snapshot(
        &restarted,
        &native_session,
        clock.unix_ms(),
        3,
        "completed",
        true,
        true,
    );
    let completion_path = publish_valid_completion(&restarted, "recorded recovered response");
    restarted.tick_receiver();

    let completed = db.receiver_job(stalled.job_id()).unwrap().unwrap();
    assert!(
        completed.state() == ReceiverJobState::AnswerReady,
        "{kind:?} recovery recorded the wrong answer-ready state"
    );
    assert!(
        completed.observation_revision() == 3,
        "frontend recovery changed the observation revision"
    );
    assert!(!completion_path.exists(), "{kind:?}");
    assert!(
        restarted.brain.receiver_run_observations().is_empty(),
        "{kind:?}"
    );
    restarted.tick_receiver();
    assert_ne!(
        db.receiver_job(later.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued,
        "{kind:?} recovery reconstruction left FIFO waiting on the departed App"
    );
    assert!(
        restarted
            .receiver
            .active_durable_run()
            .is_some_and(|run| run.claim.job().id() == later.job_id()),
        "{kind:?} recovery reconstruction launched the wrong FIFO follower"
    );

    drop(codex_override);
    if kind == AgentKind::Codex {
        std::fs::remove_dir_all(codex_sessions_dir).expect("remove Codex recovery matrix");
    }
}

const fn reconstructed_default(persisted: AgentKind) -> AgentKind {
    match persisted {
        AgentKind::Claude => AgentKind::Codex,
        AgentKind::Codex | AgentKind::OpenCode => AgentKind::Claude,
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
