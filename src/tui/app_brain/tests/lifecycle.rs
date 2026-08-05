use super::*;

#[test]
fn controller_drives_interactive_submit_queued_work_and_single_shutdown() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let scope = SessionScope::new(
        AgentKind::Claude,
        app.command_context.workspace.id(),
        app.interactive_actor.clone(),
    );
    let session = AgentSession::new("active-turn-session").expect("session");
    SessionStore::register(&app.db, &session, &app.instance, 42, &scope).expect("register session");
    SessionStore::mark_completed(&app.db, &session, &scope).expect("complete session");
    let (controller, recording) = recording_controller(&app, true, "final snapshot");
    app.brain = Some(controller);
    app.focus = Panel::Brain;

    let enter =
        crossterm::event::KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    handle_brain_key(&mut app, &enter, false);
    assert_eq!(
        SessionStore::completion_status(&app.db, &session, &scope),
        Some(crate::agent::CompletionStatus::Active)
    );
    SessionStore::mark_completed(&app.db, &session, &scope).expect("complete session again");
    app.send_brain_prompt("queued inbound work");
    assert_eq!(
        SessionStore::completion_status(&app.db, &session, &scope),
        Some(crate::agent::CompletionStatus::Active)
    );

    assert_eq!(
        recording.events(),
        vec![
            ControllerEvent::SubmitNow,
            ControllerEvent::QueueAfterActiveTurn,
        ]
    );

    app.tick_agent_controllers();
    app.tick_agent_controllers();
    app.close_brain();
    app.close_brain();

    assert_eq!(
        recording.events(),
        vec![
            ControllerEvent::SubmitNow,
            ControllerEvent::QueueAfterActiveTurn,
            ControllerEvent::QueueDelivered,
            ControllerEvent::Shutdown,
        ]
    );
}

#[test]
fn agent_exit_closes_only_the_panel_and_returns_to_the_live_tui() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (controller, recording) = recording_controller(&app, false, "final snapshot");
    app.brain = Some(controller);
    app.focus = Panel::Brain;

    assert!(app.close_exited_brain_panel());

    assert!(app.brain.is_none());
    assert_eq!(app.focus, Panel::Tasks);
    assert_eq!(recording.events(), vec![ControllerEvent::Shutdown]);
}

#[test]
fn close_delivers_transport_snapshot_with_the_initiating_actor_and_channel() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let initiating_actor = sms_actor();
    let (controller, _) = recording_controller_for_actor(
        &app,
        initiating_actor.clone(),
        false,
        "remote transport snapshot",
    );
    app.brain = Some(controller);
    app.session_actor = Some(app.interactive_actor.clone());
    app.receiver_session_id = Some("receiver-session".to_owned());
    app.receiver_started = Some(std::time::Instant::now());
    app.receiver_sender = Some("+15551234567".to_owned());
    app.receiver_lease = Some(crate::tui::receiver_state::renew(
        Channel::Email,
        0,
        std::time::Instant::now(),
    ));
    let mut delivered = None;

    app.close_brain_with(|_, completion| delivered = Some(completion));

    let delivered = delivered.expect("completion delivered before teardown");
    let (snapshot, actor, channel) = delivered.into_parts();
    assert_eq!(snapshot, "remote transport snapshot");
    assert_eq!(actor, initiating_actor);
    assert_eq!(channel, Channel::Sms);
    assert!(app.brain.is_none());
    assert_eq!(app.focus, Panel::Tasks);
}

#[test]
fn close_brain_releases_each_frontend_session_for_the_next_shell() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);

    for agent_kind in [AgentKind::Claude, AgentKind::Codex] {
        let mut app = test_app(&temporary, &cli, agent_kind);
        let scope = SessionScope::new(
            agent_kind,
            app.command_context.workspace.id(),
            app.interactive_actor.clone(),
        );
        let session_id = format!("{agent_kind:?}-session");
        app.db
            .register_scoped_fresh(&session_id, &app.instance, 42, &scope)
            .expect("register locked session");
        let live = live_panel(app.command_context.workspace.root());
        app.brain = Some(panel_controller(&app, live));
        app.focus = Panel::Brain;

        app.close_brain();

        assert!(app.brain.is_none());
        assert_eq!(app.focus, Panel::Tasks);
        assert_eq!(app.db.sessions_by_recency(&scope), [session_id]);
    }
}

#[test]
fn half_page_scroll_targets_the_visible_triage_controller() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (main, main_recording) = recording_controller(&app, true, "main");
    let (triage, triage_recording) = recording_controller(&app, true, "triage");
    app.brain = Some(main);
    app.triage_brain = Some(triage);
    app.active_brain_tab = BrainTab::Triage;
    app.focus = Panel::Brain;

    app.scroll_focused_half_page(true);
    app.scroll_focused_half_page(false);

    assert_eq!(main_recording.events(), Vec::<ControllerEvent>::new());
    assert_eq!(
        triage_recording.events(),
        vec![
            ControllerEvent::ScrollUp(20),
            ControllerEvent::ScrollDown(20)
        ]
    );
}

#[test]
fn whole_shell_shutdown_explicitly_stops_every_agent_controller_once() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (main, main_recording) = recording_controller(&app, true, "main");
    let (triage, triage_recording) = recording_controller(&app, true, "triage");
    app.brain = Some(main);
    app.triage_brain = Some(triage);

    app.shutdown_agent_controllers();
    app.shutdown_agent_controllers();

    assert_eq!(main_recording.events(), vec![ControllerEvent::Shutdown]);
    assert_eq!(triage_recording.events(), vec![ControllerEvent::Shutdown]);
}
