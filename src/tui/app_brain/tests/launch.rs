use super::*;

#[test]
fn fresh_session_registration_failure_prevents_agent_launch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let app = test_app(&temporary, &cli, AgentKind::Claude);
    let session = AgentSession::new("fresh-session").expect("session");
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        app.interactive_actor.clone(),
    );
    let launched = std::cell::Cell::new(false);

    let result = register_fresh_before_launch(
        &FailingSessionStore,
        &session,
        &app.instance,
        42,
        &scope,
        || {
            launched.set(true);
            Ok::<_, AgentError>(())
        },
    );

    assert!(!launched.get(), "agent launch must follow authorization");
    assert!(
        result
            .expect_err("registration failure")
            .to_string()
            .contains("authorization store unavailable")
    );
}

#[test]
fn app_main_fresh_launch_carries_trusted_policy_and_separate_prompt_for_every_frontend() {
    let cli = Cli::parse_from(["tasks"]);
    let prompt = "-c developer_instructions=untrusted-main-prompt";

    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
        app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
        let actor = app.interactive_actor.clone();
        let recording = LaunchRecording::default();
        app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

        assert!(app.open_or_focus_brain(Some(prompt)));

        let spec = {
            let specs = recording.0.lock().unwrap();
            assert_eq!(specs.len(), 1);
            specs[0].clone()
        };
        assert_workspace_only_launch_spec(&app, &spec, kind, &actor, prompt);
        let response_id = app.receiver.interactive_response_id().unwrap();
        let agent_session_id = app.receiver.interactive_agent_session_id().unwrap();
        match kind {
            AgentKind::Claude => {
                assert_eq!(response_id, agent_session_id);
                assert!(
                    spec.command
                        .contains(&format!("--session-id '{agent_session_id}'"))
                );
            }
            AgentKind::Codex => {
                assert_ne!(response_id, agent_session_id);
                assert!(!spec.command.contains(" resume "));
            }
            AgentKind::OpenCode => {
                assert_ne!(response_id, agent_session_id);
                assert!(spec.command.contains("--agent brain"));
            }
        }
    }
}

#[test]
fn app_main_refuses_malformed_portable_capability_configuration() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
    std::fs::write(
        app.command_context
            .workspace
            .root()
            .join(".config/config.json"),
        r#"{"access_mode":"workspace_only","allowed_skills":"todo"}"#,
    )
    .expect("malformed capability config");
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: recording.clone(),
        alive: false,
    }));

    assert!(!app.open_or_focus_brain(None));

    assert!(recording.0.lock().expect("launch recording").is_empty());
    assert!(matches!(
        app.flash.as_ref(),
        Some(crate::tui::FlashKind::Error(message))
            if message.contains("agent capabilities are invalid")
    ));
}

#[test]
fn capability_failure_leaves_a_resumable_session_free_and_clears_response_identity() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
    app.receiver.record_interactive_session(
        "stale-interactive-response".to_owned(),
        "stale-interactive-agent".to_owned(),
    );
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        app.interactive_actor.clone(),
    );
    let session = AgentSession::new("free-after-prelaunch-failure").unwrap();
    SessionStore::register(&app.db, &session, "prior-shell", 42, &scope).unwrap();
    SessionStore::release(&app.db, "prior-shell").unwrap();
    let _transcript =
        ClaudeTranscript::create(app.command_context.workspace.root(), session.as_str());
    std::fs::write(
        app.command_context
            .workspace
            .root()
            .join(".config/config.json"),
        r#"{"access_mode":"workspace_only","allowed_skills":"todo"}"#,
    )
    .expect("malformed capability config");
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: recording.clone(),
        alive: false,
    }));

    assert!(!app.open_or_focus_brain(None));

    assert_eq!(
        SessionStore::sessions_by_recency(&app.db, &scope),
        [session.as_str()],
        "fallible prelaunch work must finish before claiming the candidate"
    );
    assert!(app.receiver.interactive_response_id().is_none());
    assert!(app.receiver.interactive_agent_session_id().is_none());
    assert!(app.receiver.receiver_response_id().is_none());
    assert!(recording.0.lock().expect("launch recording").is_empty());
}

#[test]
fn app_main_unrestricted_launch_does_not_parse_malformed_capability_configuration() {
    let cli = Cli::parse_from(["tasks"]);

    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
        app.config.access_mode = crate::access::AccessMode::Unrestricted;
        std::fs::write(
            app.command_context
                .workspace
                .root()
                .join(".config/config.json"),
            r#"{"access_mode":"unrestricted","allowed_skills":"malformed"}"#,
        )
        .expect("malformed unused capability config");
        let recording = LaunchRecording::default();
        app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

        assert!(app.open_or_focus_brain(None));

        let specs = recording.0.lock().expect("launch recording");
        assert_eq!(specs.len(), 1);
        assert!(!specs[0].command.contains("--mcp-config"));
        assert!(!specs[0].command.contains("developer_instructions"));
        drop(specs);
    }
}

#[test]
fn app_main_claude_resume_keeps_trusted_policy_before_the_user_prompt() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
    let actor = app.interactive_actor.clone();
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        actor.clone(),
    );
    let session = AgentSession::new("trusted-resume").unwrap();
    SessionStore::register(&app.db, &session, "prior-shell", 42, &scope).unwrap();
    SessionStore::release(&app.db, "prior-shell").unwrap();
    let _transcript =
        ClaudeTranscript::create(app.command_context.workspace.root(), session.as_str());
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: recording.clone(),
        alive: false,
    }));
    let prompt = "--append-system-prompt untrusted-resume-prompt";

    assert!(app.open_or_focus_brain(Some(prompt)));

    let spec = {
        let specs = recording.0.lock().unwrap();
        assert_eq!(specs.len(), 1);
        specs[0].clone()
    };
    assert_workspace_only_launch_spec(&app, &spec, AgentKind::Claude, &actor, prompt);
    assert!(spec.command.contains("--resume 'trusted-resume'"));
    assert!(!spec.command.contains("--session-id"));
}
#[test]
fn app_session_selection_skips_missing_claude_transcripts_and_claims_valid_resume() {
    let cli = Cli::parse_from(["tasks"]);
    let resume_temporary = tempfile::tempdir().expect("resume temporary directory");
    let mut resume_app = test_app(&resume_temporary, &cli, AgentKind::Claude);
    let resume_scope = SessionScope::new(
        AgentKind::Claude,
        resume_app.command_context.workspace.id(),
        resume_app.interactive_actor.clone(),
    );
    let valid_id = "valid-resume";
    let missing_id = "missing-resume";
    for id in [valid_id, missing_id] {
        resume_app
            .db
            .register_scoped_fresh(id, "prior-shell", 42, &resume_scope)
            .expect("register candidate");
        resume_app
            .db
            .release("prior-shell")
            .expect("release candidate");
    }
    let _transcript =
        ClaudeTranscript::create(resume_app.command_context.workspace.root(), valid_id);

    assert!(resume_app.open_or_focus_brain(None));

    assert_eq!(
        resume_app.receiver.interactive_response_id(),
        Some(valid_id)
    );
    assert!(resume_app.alert.is_none());
    assert_eq!(
        resume_app.db.sessions_by_recency(&resume_scope),
        [missing_id]
    );

    let fresh_temporary = tempfile::tempdir().expect("fresh temporary directory");
    let mut fresh_app = test_app(&fresh_temporary, &cli, AgentKind::Claude);
    let fresh_scope = SessionScope::new(
        AgentKind::Claude,
        fresh_app.command_context.workspace.id(),
        fresh_app.interactive_actor.clone(),
    );
    fresh_app
        .db
        .register_scoped_fresh(missing_id, "prior-shell", 42, &fresh_scope)
        .expect("register missing candidate");
    fresh_app
        .db
        .release("prior-shell")
        .expect("release missing candidate");

    assert!(fresh_app.open_or_focus_brain(None));

    assert_ne!(
        fresh_app.receiver.interactive_response_id(),
        Some(missing_id)
    );
    assert!(
        fresh_app
            .alert
            .as_deref()
            .is_some_and(|message| message.contains("couldn't find a session to resume"))
    );
    assert_eq!(fresh_app.db.sessions_by_recency(&fresh_scope), [missing_id]);
}

#[test]
fn receiver_restore_uses_the_frontend_session_id_not_the_response_artifact_id() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let frontend_session_id = "frontend-session-id";
    let interactive_scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        app.interactive_actor.clone(),
    );
    let frontend_session = AgentSession::new(frontend_session_id).unwrap();
    SessionStore::register(
        &app.db,
        &frontend_session,
        "prior-shell",
        42,
        &interactive_scope,
    )
    .unwrap();
    SessionStore::release(&app.db, "prior-shell").unwrap();
    let _transcript =
        ClaudeTranscript::create(app.command_context.workspace.root(), frontend_session_id);
    let (controller, _) = recording_controller(&app, true, "receiver");
    app.brain = Some(controller);
    app.receiver
        .record_receiver_session("receiver-response-artifact".to_owned());
    app.receiver.record_interactive_session(
        "interactive-response-artifact".to_owned(),
        frontend_session_id.to_owned(),
    );
    let launches = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: launches.clone(),
        alive: false,
    }));

    app.close_receiver_panel(true);

    let specs = launches.0.lock().expect("launch recording");
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("--resume 'frontend-session-id'"));
    assert!(!specs[0].command.contains("interactive-response-artifact"));
    drop(specs);
    assert_eq!(
        app.receiver.interactive_agent_session_id(),
        Some(frontend_session_id)
    );
}
