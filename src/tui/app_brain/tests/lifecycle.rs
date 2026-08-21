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
            ControllerEvent::QueueDelivered,
        ]
    );

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
fn closing_an_opencode_panel_refreshes_the_frontend_rotated_session_id() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let recording = LaunchRecording::default();
    app.brain_transport_override = Some(Box::new(LaunchRecordingTransport {
        recording,
        alive: false,
    }));
    assert!(app.open_or_focus_brain(None));
    let placeholder = app
        .interactive_agent_session_id
        .clone()
        .expect("fresh placeholder");
    let rotated = "opencode-real-session";
    let connection = rusqlite::Connection::open(&app.db_path).expect("state connection");
    connection
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1
             WHERE agent_kind = 'opencode' AND agent_session_id = ?2
               AND brain_instance_id = ?3 AND locked_pid IS NOT NULL",
            rusqlite::params![rotated, placeholder, app.instance],
        )
        .expect("simulate lifecycle rotation");
    drop(connection);

    app.close_brain();

    assert_eq!(app.interactive_agent_session_id.as_deref(), Some(rotated));
}

#[test]
fn opencode_new_session_input_and_plugin_event_rotate_the_app_to_the_new_root() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let recording = TransportRecording::default();
    app.brain_transport_override = Some(recording.transport());
    assert!(app.open_or_focus_brain(None));
    let pending = app
        .interactive_agent_session_id
        .clone()
        .expect("pending frontend session");

    assert!(app.handle_new_session_shortcut(KeyCode::Char('n'), true));
    assert_eq!(recording.inputs(), [b"/new\r".to_vec()]);
    run_new_session_plugin_bridge(&app);

    app.close_brain();

    assert_eq!(
        app.interactive_agent_session_id.as_deref(),
        Some("root-after-new")
    );
    let connection = rusqlite::Connection::open(&app.db_path).expect("state connection");
    let prior_lock: Option<i64> = connection
        .query_row(
            "SELECT locked_pid FROM brain_sessions
             WHERE agent_kind = 'opencode' AND agent_session_id = ?1",
            [pending],
            |row| row.get(0),
        )
        .expect("prior session row");
    assert_eq!(prior_lock, None);
    assert_eq!(recording.shutdowns(), 1);
}

#[test]
fn agent_exit_closes_only_the_panel_and_returns_to_the_live_tui() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let main_recording = TransportRecording::default();
    app.brain_transport_override = Some(main_recording.transport());
    assert!(app.open_or_focus_brain(None));
    let triage_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(triage_recording.transport());
    app.open_triage_tab();
    assert_eq!(
        app.brain.as_ref().map(AgentController::kind),
        Some(AgentKind::OpenCode)
    );
    assert_eq!(
        app.active_brain_controller().map(AgentController::kind),
        Some(AgentKind::OpenCode)
    );
    main_recording.set_alive(false);
    app.focus = Panel::Brain;

    assert!(app.close_exited_brain_panel());

    assert!(app.brain.is_none());
    assert!(app.has_skill_session(crate::skill_session::SkillSessionKey::DailyTriage));
    assert_eq!(app.focus, Panel::Tasks);
    assert_eq!(main_recording.shutdowns(), 1);
    assert_eq!(triage_recording.shutdowns(), 0);
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

    for agent_kind in AgentKind::ALL {
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
fn half_page_scroll_targets_the_visible_skill_session_controller() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (main, main_recording) = recording_controller(&app, true, "main");
    let (triage, triage_recording) = recording_controller(&app, true, "triage");
    app.brain = Some(main);
    app.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "token-scroll-test",
        triage,
    );
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
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let main_recording = TransportRecording::default();
    app.brain_transport_override = Some(main_recording.transport());
    assert!(app.open_or_focus_brain(None));
    let triage_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(triage_recording.transport());
    app.open_triage_tab();
    assert_eq!(
        app.brain.as_ref().map(AgentController::kind),
        Some(AgentKind::OpenCode)
    );
    assert_eq!(
        app.active_brain_controller().map(AgentController::kind),
        Some(AgentKind::OpenCode)
    );

    app.shutdown_agent_controllers();
    app.shutdown_agent_controllers();

    assert_eq!(main_recording.shutdowns(), 1);
    assert_eq!(triage_recording.shutdowns(), 1);
}

fn run_new_session_plugin_bridge(app: &App) {
    let hook_directory = app.command_context.workspace.root().join(".brain/hooks");
    std::fs::create_dir_all(&hook_directory).expect("generic hook directory");
    for name in ["agent_session_start_hook.py", "agent_session_stop_hook.py"] {
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join(name),
            hook_directory.join(name),
        )
        .expect("copy generic lifecycle hook");
    }
    let runtime = ["bun", "node"]
        .into_iter()
        .find(|candidate| {
            std::process::Command::new(candidate)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .expect("OpenCode plugin acceptance requires Bun or Node");
    let response_id = app
        .interactive_session_id
        .as_deref()
        .expect("interactive response id");
    let output = std::process::Command::new(runtime)
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/plugin_harness.js"),
        )
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/opencode_brain_plugin.js"))
        .arg("new_session")
        .env(
            "BRAIN_WORKSPACE_ID",
            app.command_context.workspace.id().to_string(),
        )
        .env(
            "BRAIN_WORKSPACE",
            app.command_context.workspace.name().as_str(),
        )
        .env("BRAIN_ROOT", app.command_context.workspace.root())
        .env("BRAIN_ACTOR_ID", app.interactive_actor.user_id().as_str())
        .env("BRAIN_CHANNEL", "interactive")
        .env("BRAIN_AGENT_KIND", "opencode")
        .env("BRAIN_INSTANCE_ID", &app.instance)
        .env("BRAIN_PID", std::process::id().to_string())
        .env("BRAIN_STATE_DB", &app.db_path)
        .env(
            "BRAIN_RESPONSE_DIR",
            app.command_context.workspace.paths().responses_dir(),
        )
        .env("BRAIN_RESPONSE_ID", response_id)
        .output()
        .expect("run OpenCode new-session plugin harness");
    assert!(
        output.status.success(),
        "plugin harness failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
