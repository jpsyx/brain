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
fn ctrl_n_targets_the_active_main_or_triage_controller_including_triage_only() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (main, main_recording) = recording_controller(&app, true, "main");
    let (triage, triage_recording) = recording_controller(&app, true, "triage");
    app.brain = Some(main);
    app.triage_brain = Some(triage);

    app.active_brain_tab = BrainTab::Main;
    assert!(app.handle_new_session_shortcut(KeyCode::Char('n'), true));
    assert_eq!(
        main_recording.events(),
        vec![ControllerEvent::StartNewSession]
    );
    assert!(triage_recording.events().is_empty());

    app.active_brain_tab = BrainTab::Triage;
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
    triage_only.triage_brain = Some(triage);
    triage_only.active_brain_tab = BrainTab::Triage;

    assert!(triage_only.handle_new_session_shortcut(KeyCode::Char('n'), true));
    assert_eq!(recording.events(), vec![ControllerEvent::StartNewSession]);
}
