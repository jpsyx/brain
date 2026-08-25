use super::receiver_durable_support::accept_email_job_in_thread;
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn lifecycle_completion_persists_the_exact_native_session_for_the_next_message() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let first = accept_email_job_in_thread(
            &app,
            &db,
            &format!("lifecycle-continuity-{kind:?}"),
            "first message",
            100,
        );
        app.brain
            .replace_receiver_transport(TransportRecording::default().transport());

        app.tick_receiver();

        let registered = app
            .receiver
            .active_durable_run()
            .expect("active receiver")
            .attribution
            .registered_session()
            .clone();
        let native = match kind {
            AgentKind::Claude => {
                AgentSession::new("native-claude-lifecycle").expect("rotated Claude session")
            }
            AgentKind::Codex => {
                AgentSession::new(uuid::Uuid::new_v4().to_string()).expect("rotated Codex session")
            }
            AgentKind::OpenCode => {
                AgentSession::new("session-1").expect("rotated OpenCode session")
            }
        };
        assert_ne!(
            native, registered,
            "fresh runs start from a launch placeholder"
        );
        rotate_active_session(&app, &native);
        write_completed_snapshot(&app, &native, 1_200);

        app.tick_receiver();

        assert_eq!(
            db.receiver_job(first.job_id()).unwrap().unwrap().state(),
            ReceiverJobState::Done,
            "{kind:?}"
        );
        assert_eq!(
            db.receiver_conversation(first.conversation_id())
                .unwrap()
                .unwrap()
                .binding()
                .map(|binding| (binding.frontend(), binding.native_session_id().to_owned())),
            Some((kind, native.as_str().to_owned())),
            "{kind:?} lifecycle completion must preserve its exact native binding"
        );

        let _claude_transcript = (kind == AgentKind::Claude)
            .then(|| ClaudeTranscript::create(app.context.workspace().root(), native.as_str()));
        let codex_sessions_dir = temporary.path().join("codex-sessions");
        if kind == AgentKind::Codex {
            let day = codex_sessions_dir.join("9999/12/31");
            std::fs::create_dir_all(&day).expect("Codex rollout directory");
            std::fs::write(
                day.join(format!(
                    "rollout-9999-12-31T00-00-00-{}.jsonl",
                    native.as_str()
                )),
                "{}\n",
            )
            .expect("Codex rollout");
        }
        let _codex_override = (kind == AgentKind::Codex)
            .then(|| crate::agent::override_codex_sessions_dir_for_test(&codex_sessions_dir));
        let second = accept_email_job_in_thread(
            &app,
            &db,
            &format!("lifecycle-continuity-{kind:?}"),
            "second message",
            200,
        );
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());

        app.tick_receiver();

        let active = app.receiver.active_durable_run().expect("resumed receiver");
        assert_eq!(active.claim.job().id(), second.job_id(), "{kind:?}");
        assert_eq!(
            active.attribution.registered_session(),
            &native,
            "{kind:?} must resume the exact lifecycle-completed session"
        );
        let command = &transport.launch_specs()[0].command;
        match kind {
            AgentKind::Claude => assert!(command.contains("--resume"), "{command}"),
            AgentKind::Codex => assert!(command.contains("resume"), "{command}"),
            AgentKind::OpenCode => assert!(command.contains("--session"), "{command}"),
        }
    }
}

#[test]
fn lifecycle_completion_stays_retryable_when_native_binding_persistence_fails() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job_in_thread(
        &app,
        &db,
        "lifecycle-binding-failure",
        "complete after storage recovers",
        100,
    );
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let native = AgentSession::new("session-1").expect("native OpenCode session");
    rotate_active_session(&app, &native);
    let snapshot = write_completed_snapshot(&app, &native, 1_200);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("binding fault connection")
        .execute_batch(
            "CREATE TRIGGER fail_lifecycle_receiver_binding
             BEFORE UPDATE OF agent_session_id ON receiver_conversations
             BEGIN
               SELECT RAISE(FAIL, 'injected lifecycle binding failure');
             END;",
        )
        .expect("install deterministic binding failure");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched,
        "terminal evidence must remain retryable when continuity cannot be persisted"
    );
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_none()
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(transport.shutdowns(), 0);
    assert!(snapshot.exists());
}

fn rotate_active_session(app: &App, session: &AgentSession) {
    let active = app.receiver.active_durable_run().expect("active receiver");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![session.as_str(), active.attribution.instance()],
        )
        .expect("simulate lifecycle native session");
}

fn write_completed_snapshot(
    app: &App,
    session: &AgentSession,
    completed_at_unix_ms: u64,
) -> PathBuf {
    let active = app.receiver.active_durable_run().expect("active receiver");
    let instance = active.attribution.instance();
    let path = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "revision": 1,
            "phase": "completed",
            "job_token": active.claim.job().token().to_string(),
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": null,
            "accepted_at_unix_ms": null,
            "progressing_at_unix_ms": null,
            "completed_at_unix_ms": completed_at_unix_ms,
        })
        .to_string(),
    )
    .expect("observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
    path
}
