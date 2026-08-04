use super::*;

#[test]
fn receiver_queue_reuses_the_matching_warm_session_through_app_dispatch() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let actor = app.interactive_actor.clone();
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        actor.clone(),
    );
    let session = AgentSession::new("warm-receiver-session").expect("session");
    SessionStore::register(&app.db, &session, &app.instance, 42, &scope).expect("register session");
    SessionStore::mark_completed(&app.db, &session, &scope).expect("complete session");
    let live = live_panel(app.command_context.workspace.root());
    app.brain = Some(panel_controller(&app, live));
    app.session_actor = Some(actor.clone());
    app.receiver_session_id = Some("receiver-session".to_owned());
    app.receiver_lease = Some(crate::tui::receiver_state::renew(
        Channel::Sms,
        0,
        std::time::Instant::now(),
    ));
    app.receiver_queue.push(InboundMessage {
        workspace_id: app.command_context.workspace.id(),
        actor: actor.clone(),
        channel: Channel::Sms,
        body: "continue this conversation".to_owned(),
        sender: "+15551234567".to_owned(),
        participants: vec!["+15551234567".to_owned()],
        provider_id: Some("provider-message-1".to_owned()),
        attachments: Vec::new(),
    });

    app.tick_receiver();

    assert!(app.receiver_queue.is_empty());
    assert_eq!(app.receiver_session_id.as_deref(), Some("receiver-session"));
    assert_eq!(app.session_actor.as_ref(), Some(&actor));
    assert!(app.receiver_started.is_some());
    assert!(app.brain_turn_active);
    assert_eq!(
        SessionStore::completion_status(&app.db, &session, &scope),
        Some(crate::agent::CompletionStatus::Active)
    );
}
