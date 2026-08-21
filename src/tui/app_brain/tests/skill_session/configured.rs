use super::*;

#[test]
fn a_configured_skill_session_launches_its_own_prompt_under_its_own_title() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.set_test_configured_skill_sessions(serde_json::json!([{
        "title": "Email triage",
        "prompt": "/email-triage",
        "command_label": "Run email triage",
    }]));
    let recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(recording.transport());

    app.run_skill_session(SkillSessionKey::Custom(0));

    assert!(app.has_skill_session(SkillSessionKey::Custom(0)));
    assert_eq!(
        app.brain_tab_titles(),
        vec!["Brain".to_owned(), "Email triage".to_owned()]
    );
    let specs = recording.launch_specs();
    assert_eq!(specs.len(), 1);
    // The workspace's prompt reaches the session, and so does the completion
    // protocol brain appends — the skill itself knows nothing about brain.
    assert!(specs[0].command.contains("/email-triage"), "{:?}", specs[0]);
    assert!(
        specs[0]
            .command
            .contains(crate::skill_session::prompt::DONE_URL_ENV),
        "{:?}",
        specs[0]
    );
    assert!(
        specs[0]
            .environment
            .iter()
            .any(|(name, _)| name == crate::skill_session::prompt::TOKEN_ENV)
    );
}

#[test]
fn two_skill_sessions_run_as_separate_tabs_and_complete_independently() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.set_test_configured_skill_sessions(serde_json::json!([{
        "title": "Email triage",
        "prompt": "/email-triage",
        "command_label": "Run email triage",
    }]));

    let triage_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(triage_recording.transport());
    app.open_triage_tab();

    let email_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(email_recording.transport());
    app.run_skill_session(SkillSessionKey::Custom(0));

    // Both run at once, each with its own tab in open order.
    assert_eq!(
        app.brain_tab_titles(),
        vec![
            "Brain".to_owned(),
            "Daily triage".to_owned(),
            "Email triage".to_owned()
        ]
    );
    let (runnable, open) = app.skill_session_palette_rows();
    assert!(
        runnable.is_empty(),
        "a running session must offer no start row: {runnable:?}"
    );
    assert_eq!(open.len(), 2);

    // Only the session whose token arrives closes; the other keeps running.
    let email_token = app
        .skill_session_token(SkillSessionKey::Custom(0))
        .expect("email session token");
    crate::skill_session::signal::record_done(&app.command_context.workspace, &email_token, &[])
        .expect("completion signal");
    app.tick_skill_sessions();

    assert!(app.has_skill_session(SkillSessionKey::DailyTriage));
    assert!(!app.has_skill_session(SkillSessionKey::Custom(0)));
    assert_eq!(email_recording.shutdowns(), 1);
    assert_eq!(triage_recording.shutdowns(), 0);
    // With it closed, its start row is offered again.
    let (runnable, _) = app.skill_session_palette_rows();
    assert_eq!(
        runnable,
        vec![(SkillSessionKey::Custom(0), "Run email triage".to_owned())]
    );
}

#[test]
fn a_declared_required_output_holds_the_tab_open_until_it_exists() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(recording.transport());
    app.open_triage_tab();

    let token = app
        .skill_session_token(SkillSessionKey::DailyTriage)
        .expect("session token");
    let required = temporary.path().join("declared-output.pdf");
    crate::skill_session::signal::record_done(
        &app.command_context.workspace,
        &token,
        &[required.display().to_string()],
    )
    .expect("completion signal");

    app.tick_skill_sessions();
    assert!(
        app.has_skill_session(SkillSessionKey::DailyTriage),
        "a premature signal must not close the tab before declared outputs land"
    );

    std::fs::write(&required, b"output").expect("write declared output");
    app.tick_skill_sessions();
    assert!(!app.has_skill_session(SkillSessionKey::DailyTriage));
}

#[test]
fn the_builtin_daily_triage_session_is_offered_only_while_the_check_is_enabled() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.skip_daily_triage_check = false;

    let keys: Vec<_> = app
        .available_skill_sessions()
        .into_iter()
        .map(|spec| spec.key)
        .collect();
    assert_eq!(keys, vec![SkillSessionKey::DailyTriage]);

    // Silencing the daily-triage check (config-seeded, palette-toggled) also
    // withdraws its builtin session — the workspace has turned the feature off.
    app.skip_daily_triage_check = true;
    assert!(app.available_skill_sessions().is_empty());
}

#[test]
fn a_stale_signal_from_a_dead_shell_cannot_close_a_freshly_opened_tab() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    crate::skill_session::signal::record_done(
        &app.command_context.workspace,
        "abandoned-token",
        &[],
    )
    .expect("stale signal");

    let recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(recording.transport());
    app.open_triage_tab();
    app.tick_skill_sessions();

    assert!(app.has_skill_session(SkillSessionKey::DailyTriage));
    assert_eq!(recording.shutdowns(), 0);
}

#[test]
fn closing_one_session_leaves_another_tab_selected_rather_than_jumping_to_main() {
    // With several tabs open, a background session finishing must not yank the
    // user off the tab they are reading.
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.set_test_configured_skill_sessions(serde_json::json!([
        {"title": "Email triage", "prompt": "/email-triage"},
    ]));

    let triage_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(triage_recording.transport());
    app.open_triage_tab();
    let email_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(email_recording.transport());
    app.run_skill_session(SkillSessionKey::Custom(0));

    // Watching daily triage (tab 2) while email triage (tab 3) completes.
    app.select_brain_tab_slot(1);
    let watched = app.active_brain_tab;
    let email_token = app
        .skill_session_token(SkillSessionKey::Custom(0))
        .expect("email session token");
    crate::skill_session::signal::record_done(&app.command_context.workspace, &email_token, &[])
        .expect("completion signal");

    app.tick_skill_sessions();

    assert_eq!(
        app.active_brain_tab, watched,
        "closing another tab must not change which tab is showing"
    );
    assert_eq!(app.focus, Panel::Brain);
}

#[test]
fn a_failed_start_leaves_you_on_the_tab_you_were_reading() {
    // A session that never launched was never selected, so the failure must not
    // move the panel off whatever tab is showing.
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.set_test_configured_skill_sessions(serde_json::json!([
        {"title": "Email triage", "prompt": "/email-triage"},
    ]));
    let triage_recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(triage_recording.transport());
    app.open_triage_tab();
    let watched = app.active_brain_tab;

    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(Box::new(FailingSpawnTransport));
    app.run_skill_session(SkillSessionKey::Custom(0));

    assert!(!app.has_skill_session(SkillSessionKey::Custom(0)));
    assert_eq!(app.active_brain_tab, watched);
    assert!(app.has_skill_session(SkillSessionKey::DailyTriage));
    assert!(matches!(app.flash, Some(crate::tui::FlashKind::Error(_))));
}

#[test]
fn an_unoccupied_tab_slot_selects_nothing_so_its_keystroke_stays_ordinary_input() {
    // The macOS Option glyphs that address tabs 3..9 (`£`, `•`, …) are also
    // typeable characters. `select_brain_tab_slot` reporting false is what lets
    // the event loop forward such a keystroke instead of swallowing it.
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(recording.transport());
    app.open_triage_tab();

    assert!(app.select_brain_tab_slot(1), "the open session tab selects");
    assert!(!app.select_brain_tab_slot(2), "no third tab is open");
    assert!(!app.select_brain_tab_slot(8));
    // The selection did not move off the tab that is open.
    assert_eq!(app.active_brain_tab_index(), 1);
}
