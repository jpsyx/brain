use super::*;

#[test]
fn app_session_selection_skips_a_stale_opencode_row_and_starts_fresh() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let scope = SessionScope::new(
        AgentKind::OpenCode,
        app.command_context.workspace.id(),
        app.interactive_actor.clone(),
    );
    let stale = AgentSession::new("stale-opencode-session").unwrap();
    SessionStore::register(&app.db, &stale, "prior-shell", 42, &scope).unwrap();
    SessionStore::release(&app.db, "prior-shell").unwrap();
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording: recording.clone(),
        alive: false,
    }));

    assert!(app.open_or_focus_brain(None));

    assert_ne!(
        app.interactive_agent_session_id.as_deref(),
        Some(stale.as_str())
    );
    assert!(
        app.alert
            .as_deref()
            .is_some_and(|message| message.contains("couldn't find a session to resume"))
    );
    assert_eq!(app.db.sessions_by_recency(&scope), [stale.as_str()]);
    assert!(!recording.0.lock().unwrap()[0].command.contains("--session"));
}

#[test]
fn restarted_opencode_app_resumes_a_persisted_workspace_session() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let session = AgentSession::new("session-1").expect("fixture session");

    {
        let prior = test_app(&temporary, &cli, AgentKind::OpenCode);
        let scope = SessionScope::new(
            AgentKind::OpenCode,
            prior.command_context.workspace.id(),
            prior.interactive_actor.clone(),
        );
        SessionStore::register(&prior.db, &session, "prior-shell", 42, &scope)
            .expect("persist prior session");
        SessionStore::release(&prior.db, "prior-shell").expect("release prior shell");
    }

    let mut restarted = test_app(&temporary, &cli, AgentKind::OpenCode);
    let recording = TransportRecording::default();
    restarted.brain_transport_override = Some(recording.transport());

    assert!(restarted.open_or_focus_brain(None));

    assert_eq!(
        restarted.interactive_agent_session_id.as_deref(),
        Some("session-1")
    );
    assert!(restarted.alert.is_none());
    let specs = recording.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("--session 'session-1'"));
}

#[test]
fn opencode_receiver_restore_resumes_only_a_session_that_still_exists() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    seed_free_opencode_session(&app, "session-1");
    let current_transport = TransportRecording::default();
    current_transport.set_alive(true);
    app.brain = Some(app.controller_for_transport(sms_actor(), current_transport.transport()));
    app.receiver_session_id = Some("receiver-response".to_owned());
    app.interactive_session_id = Some("interactive-response".to_owned());
    app.interactive_agent_session_id = Some("session-1".to_owned());
    let restored = TransportRecording::default();
    app.brain_transport_override = Some(restored.transport());

    app.close_receiver_panel(true);

    assert_eq!(current_transport.shutdowns(), 1);
    assert_eq!(
        app.interactive_agent_session_id.as_deref(),
        Some("session-1")
    );
    let specs = restored.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("--session 'session-1'"));
}

#[test]
fn opencode_receiver_restore_falls_back_fresh_when_the_session_disappeared() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    seed_free_opencode_session(&app, "stale");
    let current_transport = TransportRecording::default();
    current_transport.set_alive(true);
    app.brain = Some(app.controller_for_transport(sms_actor(), current_transport.transport()));
    app.receiver_session_id = Some("receiver-response".to_owned());
    app.interactive_session_id = Some("interactive-response".to_owned());
    app.interactive_agent_session_id = Some("stale".to_owned());
    let restored = TransportRecording::default();
    app.brain_transport_override = Some(restored.transport());

    app.close_receiver_panel(true);

    assert_eq!(current_transport.shutdowns(), 1);
    assert_ne!(app.interactive_agent_session_id.as_deref(), Some("stale"));
    assert!(
        app.alert
            .as_deref()
            .is_some_and(|message| message.contains("couldn't find a session to resume"))
    );
    let specs = restored.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].command.contains("--session"));
}

fn seed_free_opencode_session(app: &App, session_id: &str) {
    let scope = SessionScope::new(
        AgentKind::OpenCode,
        app.command_context.workspace.id(),
        app.interactive_actor.clone(),
    );
    let session = AgentSession::new(session_id).expect("fixture session");
    SessionStore::register(&app.db, &session, "prior-shell", 42, &scope).expect("register session");
    SessionStore::release(&app.db, "prior-shell").expect("release session");
}
