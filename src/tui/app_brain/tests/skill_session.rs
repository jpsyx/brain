use super::*;

use crate::skill_session::SkillSessionKey;

#[test]
fn local_workspace_urls_use_the_ingress_accepted_at_registration() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let app = test_app(&temporary, &cli, AgentKind::Claude);

    assert_eq!(
        app.habits_url_for_port(4773),
        format!(
            "http://127.0.0.1:4773/local/{ACCEPTED_LOCAL_CAPABILITY}/w/{ACCEPTED_INGRESS}/habits"
        )
    );
    assert_eq!(
        app.session_done_url_for_port(4773),
        format!(
            "http://127.0.0.1:4773/local/{ACCEPTED_LOCAL_CAPABILITY}/w/{ACCEPTED_INGRESS}/session/done"
        )
    );
}

#[test]
fn open_triage_tab_launches_the_selected_ephemeral_untracked_controller() {
    let cli = Cli::parse_from(["tasks"]);

    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
        app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
        let actor = app.interactive_actor.clone();
        let recording = LaunchRecording::default();
        app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
        app.session_transport_override = Some(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

        app.open_triage_tab();

        let controller = app
            .active_brain_controller()
            .expect("skill session controller");
        assert_eq!(controller.kind(), kind);
        assert!(app.has_skill_session(SkillSessionKey::DailyTriage));
        assert!(matches!(app.active_brain_tab, BrainTab::Session(_)));
        assert_eq!(app.focus, Panel::Brain);
        let spec = {
            let specs = recording.0.lock().expect("launch recording");
            assert_eq!(specs.len(), 1);
            specs[0].clone()
        };
        assert_workspace_only_launch_spec(
            &app,
            &spec,
            kind,
            &actor,
            &crate::skill_session::prompt::launch_prompt("/triage"),
        );
        assert_eq!(
            spec.environment
                .iter()
                .find(|(name, _)| name == "BRAIN_AGENT_KIND")
                .map(|(_, value)| value.as_str()),
            Some(kind.as_str())
        );
        assert!(
            spec.environment
                .iter()
                .any(|(name, _)| name == crate::skill_session::prompt::DONE_URL_ENV)
        );
        assert!(
            spec.environment
                .iter()
                .any(|(name, _)| name == crate::skill_session::prompt::TOKEN_ENV)
        );
        for forbidden in ["BRAIN_INSTANCE_ID", "BRAIN_STATE_DB", "BRAIN_RESPONSE_ID"] {
            assert!(
                spec.environment.iter().all(|(name, _)| name != forbidden),
                "an ephemeral skill session must omit {forbidden}"
            );
        }
        let connection = rusqlite::Connection::open(&app.db_path).expect("state database");
        let registered: i64 = connection
            .query_row("SELECT COUNT(*) FROM brain_sessions", [], |row| row.get(0))
            .expect("session count");
        assert_eq!(registered, 0, "skill sessions remain untracked");
    }
}

#[test]
fn opencode_triage_completion_cleans_up_the_ephemeral_transport_and_signal_once() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    let recording = TransportRecording::default();
    app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
    app.session_transport_override = Some(recording.transport());

    app.open_triage_tab();
    let token = app
        .skill_session_token(SkillSessionKey::DailyTriage)
        .expect("session token");
    crate::skill_session::signal::record_done(&app.command_context.workspace, &token, &[])
        .expect("completion signal");

    app.tick_skill_sessions();
    app.tick_skill_sessions();

    assert!(!app.has_skill_session(SkillSessionKey::DailyTriage));
    assert_eq!(app.active_brain_tab, BrainTab::Main);
    assert_eq!(app.focus, Panel::Tasks);
    assert_eq!(recording.shutdowns(), 1);
    assert!(
        crate::skill_session::signal::read_signal(&app.command_context.workspace, &token).is_none()
    );
}

#[test]
fn skip_button_marks_managed_daily_triage_done_without_launching_an_agent() {
    let cli = Cli::parse_from(["tasks"]);
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.config.enable_triage_habits = true;

    let habits_path = app
        .command_context
        .workspace
        .root()
        .join("tasks/habits.csv");
    std::fs::write(
        &habits_path,
        "task_uuid,task_id,task_name,status,due_date,recur_interval,recur_unit,created_date,completed_date,last_touched,assigned_to,system_key\n\
         u1,H35,Morning Triage,not_started,2026-08-04,1,days,2026-08-04,,,pablo,brain.triage.daily\n",
    )
    .expect("write managed daily triage habit");
    app.reload_tasks()
        .expect("reload after seeding the managed habit");

    // Press Skip on the daily-triage nudge.
    app.confirm = Some(crate::tui::ConfirmState::run_triage(
        "H35".to_owned(),
        "Morning Triage".to_owned(),
    ));
    crate::tui::handlers::run_confirm_skip(&mut app);

    // The modal is dismissed and no agent panel was launched — Skip is a pure
    // in-process CSV mutation.
    assert!(app.confirm.is_none(), "modal should be dismissed");
    assert!(
        app.brain.is_none(),
        "Skip must not launch the main brain panel"
    );
    assert!(
        !app.has_skill_session(SkillSessionKey::DailyTriage),
        "Skip must not open a triage tab"
    );

    // Today's occurrence is completed and tomorrow's is spawned.
    let csv = std::fs::read_to_string(&habits_path).expect("read habits");
    assert!(
        csv.contains("H35,Morning Triage,done,2026-08-04"),
        "today's occurrence not marked done; got:\n{csv}"
    );
    assert!(
        csv.contains("Morning Triage,not_started,2026-08-05,1,days"),
        "next occurrence not spawned; got:\n{csv}"
    );
}

#[test]
fn unrestricted_triage_launch_does_not_parse_malformed_machine_capabilities() {
    use std::collections::{BTreeMap, BTreeSet};

    let cli = Cli::parse_from(["tasks"]);
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
        app.config.access_mode = crate::access::AccessMode::Unrestricted;
        let name = app.command_context.workspace.name().clone();
        let mut environment =
            crate::workspace::RegistryStore::load_from(app.command_context.registry_store.path())
                .expect("current registry")
                .workspaces[&name]
                .env
                .clone();
        environment.insert(
            "agent_capabilities".to_owned(),
            serde_json::json!({"mcps": "malformed"}),
        );
        app.command_context
            .registry_store
            .replace(&crate::workspace::MachineRegistry {
                schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
                default_workspace: name.clone(),
                workspaces: BTreeMap::from([(
                    name,
                    crate::workspace::WorkspaceRecord {
                        workspace_id: app.command_context.workspace.id(),
                        root: app.command_context.workspace.root().to_path_buf(),
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: environment,
                    },
                )]),
                env: serde_json::Map::new(),
            })
            .expect("malformed unused machine capabilities");
        let recording = LaunchRecording::default();
        app.session_done_url_override = Some("http://127.0.0.1:4773/session/done".to_owned());
        app.session_transport_override = Some(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

        app.open_triage_tab();

        assert!(app.has_skill_session(SkillSessionKey::DailyTriage));
        let specs = recording.0.lock().expect("launch recording");
        assert_eq!(specs.len(), 1);
        assert!(!specs[0].command.contains("--mcp-config"));
        assert!(!specs[0].command.contains("developer_instructions"));
        drop(specs);
    }
}

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
