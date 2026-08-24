use super::*;

#[test]
fn app_session_selection_skips_a_stale_opencode_row_and_starts_fresh() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let scope = SessionScope::new(
        AgentKind::OpenCode,
        app.context.workspace().id(),
        app.brain.interactive_actor().clone(),
    );
    let stale = AgentSession::new("stale-opencode-session").unwrap();
    SessionStore::register(&app.services, &stale, "prior-shell", 42, &scope).unwrap();
    SessionStore::release(&app.services, "prior-shell").unwrap();
    let recording = LaunchRecording::default();
    app.brain
        .replace_brain_transport(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

    assert!(app.open_or_focus_brain(None));

    assert_ne!(
        app.brain.interactive_agent_session_id(),
        Some(stale.as_str())
    );
    assert!(
        app.status
            .alert()
            .is_some_and(|message| message.contains("couldn't find a session to resume"))
    );
    assert_eq!(
        SessionStore::sessions_by_recency(&app.services, &scope),
        [stale.as_str()]
    );
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
            prior.context.workspace().id(),
            prior.brain.interactive_actor().clone(),
        );
        SessionStore::register(&prior.services, &session, "prior-shell", 42, &scope)
            .expect("persist prior session");
        SessionStore::release(&prior.services, "prior-shell").expect("release prior shell");
    }

    let mut restarted = test_app(&temporary, &cli, AgentKind::OpenCode);
    let recording = TransportRecording::default();
    restarted
        .brain
        .replace_brain_transport(recording.transport());

    assert!(restarted.open_or_focus_brain(None));

    assert_eq!(
        restarted.brain.interactive_agent_session_id(),
        Some("session-1")
    );
    assert!(restarted.status.alert().is_none());
    let specs = recording.launch_specs();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.contains("--session 'session-1'"));
}
