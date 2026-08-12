use super::*;

#[test]
fn ctrl_n_routes_new_session_through_the_selected_controller_adapter() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);

    for agent_kind in AgentKind::ALL {
        let mut app = test_app(&temporary, &cli, agent_kind);
        let capture = capture_panel(app.command_context.workspace.root());
        app.brain = Some(panel_controller(&app, capture));
        assert!(
            wait_for_panel_contents(app.brain.as_ref().expect("panel"), "READY"),
            "capture panel did not become ready"
        );

        assert!(!app.handle_new_session_shortcut(KeyCode::Char('n'), false));
        assert!(app.handle_new_session_shortcut(KeyCode::Char('n'), true));
        assert_eq!(app.focus, Panel::Brain);
        assert!(app.brain_turn_active);
        let expected_bytes = match agent_kind {
            AgentKind::Claude | AgentKind::OpenCode => "2f 6e 65 77 0d",
            AgentKind::Codex => "2f 6e 65 77 09",
        };
        let panel = app
            .brain
            .as_ref()
            .expect("panel remains open until capture exits");
        assert!(
            wait_for_panel_contents(panel, expected_bytes),
            "capture panel did not receive deferred /new bytes: {}",
            panel.snapshot().expect("supported capture panel snapshot")
        );
    }
}

#[test]
fn ctrl_n_targets_the_active_main_or_skill_session_controller_including_session_only() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (main, main_recording) = recording_controller(&app, true, "main");
    let (triage, triage_recording) = recording_controller(&app, true, "triage");
    app.brain = Some(main);
    let session_tab = app.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "token-main-test",
        triage,
    );

    app.active_brain_tab = BrainTab::Main;
    assert!(app.handle_new_session_shortcut(KeyCode::Char('n'), true));
    assert_eq!(
        main_recording.events(),
        vec![ControllerEvent::StartNewSession]
    );
    assert!(triage_recording.events().is_empty());

    app.active_brain_tab = BrainTab::Session(session_tab);
    assert!(app.handle_new_session_shortcut(KeyCode::Char('n'), true));
    assert_eq!(
        main_recording.events(),
        vec![ControllerEvent::StartNewSession]
    );
    assert_eq!(
        triage_recording.events(),
        vec![ControllerEvent::StartNewSession]
    );

    let triage_only_temporary = tempfile::tempdir().expect("temporary directory");
    let mut triage_only = test_app(&triage_only_temporary, &cli, AgentKind::Claude);
    let (triage, recording) = recording_controller(&triage_only, true, "triage only");
    triage_only.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "token-session-only",
        triage,
    );

    assert!(triage_only.handle_new_session_shortcut(KeyCode::Char('n'), true));
    assert_eq!(recording.events(), vec![ControllerEvent::StartNewSession]);
}

/// A remote turn owns the panel until it answers. Local keystrokes used to be
/// forwarded straight into that PTY, landing in the composer beside the
/// injected prompt, so they are dropped for the duration of the turn. The
/// interrupt key stays live so a stuck remote turn is never a trap.
#[test]
fn local_keystrokes_do_not_reach_a_pty_that_is_answering_a_remote_message() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let recording = TransportRecording::default();
    app.brain_transport_override = Some(recording.transport());
    assert!(app.open_or_focus_brain(None));
    app.focus = Panel::Brain;
    app.receiver_started = Some(std::time::Instant::now());

    let typed =
        crossterm::event::KeyEvent::new(KeyCode::Char('x'), crossterm::event::KeyModifiers::NONE);
    let enter =
        crossterm::event::KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    handle_brain_key(&mut app, &typed, false);
    handle_brain_key(&mut app, &enter, false);
    assert_eq!(
        recording.inputs(),
        Vec::<Vec<u8>>::new(),
        "no local keystroke may join the remote conversation"
    );
    assert!(
        !app.brain_turn_active,
        "a locked-out Enter must not be recorded as starting a turn"
    );

    let interrupt =
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL);
    handle_brain_key(&mut app, &interrupt, true);
    assert_eq!(
        recording.inputs(),
        [b"\x03".to_vec()],
        "the interrupt key stays available"
    );
}
