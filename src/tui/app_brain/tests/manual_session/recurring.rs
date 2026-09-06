use super::*;

#[test]
fn main_exit_relaunches_in_place_without_moving_a_selected_additional_tab() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let clock = test_clock(&mut app);
        let main = TransportRecording::default();
        let next = TransportRecording::default();
        let atlas = TransportRecording::default();
        app.brain.replace_brain_transport(main.transport());
        assert!(app.open_or_focus_brain(None));
        app.brain.replace_manual_transport(atlas.transport());
        app.start_manual_session(name("Atlas"));
        let active = app.effective_brain_tab();
        app.focus_tasks();
        app.brain.replace_brain_transport(next.transport());
        clock.advance(std::time::Duration::from_secs(6));
        app.tick_manual_sessions();
        main.set_alive(false);

        app.tick_manual_sessions();

        assert_eq!(main.shutdowns(), 1);
        assert_eq!(next.launch_specs().len(), 1);
        assert!(app.brain.main_controller().unwrap().is_alive().unwrap());
        assert_eq!(app.effective_brain_tab(), active);
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(atlas.shutdowns(), 0);
        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap()
                .len(),
            2
        );
    }
}

#[test]
fn a_normally_exited_additional_session_removes_only_its_durable_mapping() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let clock = test_clock(&mut app);
        let atlas = TransportRecording::default();
        app.brain.replace_manual_transport(atlas.transport());
        app.start_manual_session(name("Atlas"));
        clock.advance(std::time::Duration::from_secs(6));
        app.tick_manual_sessions();
        atlas.set_alive(false);

        app.tick_manual_sessions();

        assert_eq!(atlas.shutdowns(), 1);
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert!(app.brain.any_panel_visible());
        assert!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn refused_additional_recovery_keeps_the_same_tab_after_async_startup_failure() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let prior = test_app(&temporary, &cli, kind);
        restoration::persist(
            &prior,
            &restoration::saved_record("main", "missing-main", "Brain", 0),
        );
        restoration::persist(
            &prior,
            &restoration::saved_record("atlas", "session-1", "Atlas", 1),
        );
        let _claude = (kind == AgentKind::Claude)
            .then(|| ClaudeTranscript::create(prior.context.workspace().root(), "session-1"));
        let sessions_dir = temporary.path().join("codex-sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(
            sessions_dir.join("rollout-2026-09-05T00-00-00-session-1.jsonl"),
            "{}\n",
        )
        .unwrap();
        let _codex = (kind == AgentKind::Codex)
            .then(|| crate::agent::override_codex_sessions_dir_for_test(&sessions_dir));
        let mut app = test_app(&temporary, &cli, kind);
        let clock = test_clock(&mut app);
        let main = TransportRecording::default();
        let resumed = TransportRecording::default();
        let fresh = TransportRecording::default();
        app.brain.replace_brain_transport(main.transport());
        app.brain.replace_manual_transport(resumed.transport());
        app.restore_manual_sessions();
        let tab = app.brain.session_tab_ids()[0];
        app.select_brain_tab(BrainTab::Session(tab));
        resumed.set_alive(false);
        app.brain.replace_manual_transport(fresh.transport());

        app.tick_manual_sessions();

        assert_eq!(app.brain.session_tab_ids(), [tab]);
        assert_eq!(app.effective_brain_tab(), BrainTab::Session(tab));
        assert_eq!(app.active_brain_tab_title(), Some("Atlas"));
        assert_eq!(resumed.shutdowns(), 1);
        assert_eq!(fresh.launch_specs().len(), 1);
        assert!(!fresh.launch_specs()[0].command.contains("'session-1'"));
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        assert_eq!(records[1].id.as_str(), "atlas");
        assert_eq!(records[1].position, 1);
        assert_ne!(records[1].agent_session.as_str(), "session-1");

        fresh.set_alive(false);
        app.tick_manual_sessions();
        clock.advance(std::time::Duration::from_secs(60));
        app.tick_manual_sessions();
        assert_eq!(app.brain.session_tab_ids(), [tab]);
        assert_eq!(app.effective_brain_tab(), BrainTab::Session(tab));
        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap(),
            records
        );
        assert_eq!(
            app.services
                .locked_session_for_instance("atlas", &interactive_scope(&app)),
            None
        );
        assert_eq!(fresh.shutdowns(), 1);
    }
}

#[test]
fn main_async_startup_failure_stays_visible_without_recurring_respawns() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let clock = test_clock(&mut app);
        let failed = TransportRecording::default();
        let unwanted = TransportRecording::default();
        app.brain.replace_brain_transport(failed.transport());
        assert!(app.open_or_focus_brain(None));
        app.focus_tasks();
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        failed.set_alive(false);
        app.brain.replace_brain_transport(unwanted.transport());

        app.tick_manual_sessions();
        clock.advance(std::time::Duration::from_secs(60));
        app.tick_manual_sessions();
        app.tick_manual_sessions();

        assert!(unwanted.launch_specs().is_empty());
        assert_eq!(failed.shutdowns(), 1);
        assert!(app.brain.main_controller().is_none());
        assert!(app.brain.any_panel_visible());
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap(),
            records
        );
        assert_eq!(
            app.services
                .locked_session_for_instance(app.brain.instance(), &interactive_scope(&app)),
            None
        );
    }
}

#[test]
fn a_failed_resume_replacement_keeps_its_mapping_without_retrying_every_tick() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let prior = test_app(&temporary, &cli, AgentKind::OpenCode);
    restoration::persist(
        &prior,
        &restoration::saved_record("main", "missing-main", "Brain", 0),
    );
    restoration::persist(
        &prior,
        &restoration::saved_record("atlas", "session-1", "Atlas", 1),
    );
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let main = TransportRecording::default();
    let resumed = TransportRecording::default();
    let unwanted_retry = TransportRecording::default();
    app.brain.replace_brain_transport(main.transport());
    app.brain.replace_manual_transport(resumed.transport());
    app.restore_manual_sessions();
    let tab = app.brain.session_tab_ids()[0];
    app.select_brain_tab(BrainTab::Session(tab));
    resumed.set_alive(false);
    app.brain
        .replace_manual_transport(Box::new(FailingSpawnTransport));
    app.tick_manual_sessions();
    let records = app
        .services
        .manual_sessions(&interactive_scope(&app))
        .unwrap();
    app.brain
        .replace_manual_transport(unwanted_retry.transport());

    app.tick_manual_sessions();

    assert!(unwanted_retry.launch_specs().is_empty());
    assert_eq!(
        app.services
            .manual_sessions(&interactive_scope(&app))
            .unwrap(),
        records
    );
    assert_eq!(app.brain.session_tab_ids(), [tab]);
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(tab));
    assert_eq!(app.shell.focus(), Panel::Brain);
}

#[test]
fn entering_input_on_an_additional_manual_marks_only_that_session_active() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let main = TransportRecording::default();
        let atlas = TransportRecording::default();
        app.brain.replace_brain_transport(main.transport());
        assert!(app.open_or_focus_brain(None));
        app.brain.replace_manual_transport(atlas.transport());
        app.start_manual_session(name("Atlas"));
        let scope = interactive_scope(&app);
        let records = app.services.manual_sessions(&scope).unwrap();
        for record in &records {
            SessionStore::mark_completed(&app.services, &record.agent_session, &scope).unwrap();
        }

        crate::tui::event_loop::update_application(
            &mut app,
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

        assert_eq!(
            SessionStore::completion_status(&app.services, &records[0].agent_session, &scope),
            Some(crate::agent::CompletionStatus::Completed)
        );
        assert_eq!(
            SessionStore::completion_status(&app.services, &records[1].agent_session, &scope),
            Some(crate::agent::CompletionStatus::Active)
        );
        assert!(!app.brain.turn_active());
        assert!(main.inputs().is_empty());
        assert!(!atlas.inputs().is_empty());
    }
}
