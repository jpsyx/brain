use super::*;

#[test]
fn open_triage_tab_launches_the_selected_ephemeral_untracked_controller() {
    let cli = Cli::parse_from(["tasks"]);

    for kind in [AgentKind::Claude, AgentKind::Codex] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
        app.config.access_mode = crate::access::AccessMode::WorkspaceOnly;
        let actor = app.interactive_actor.clone();
        let recording = LaunchRecording::default();
        app.triage_done_url_override = Some("http://127.0.0.1:4773/triage/done".to_owned());
        app.triage_transport_override = Some(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

        app.open_triage_tab();

        let controller = app.triage_brain.as_ref().expect("triage controller");
        assert_eq!(controller.kind(), kind);
        assert_eq!(app.active_brain_tab, BrainTab::Triage);
        assert_eq!(app.focus, Panel::Brain);
        let spec = {
            let specs = recording.0.lock().expect("launch recording");
            assert_eq!(specs.len(), 1);
            specs[0].clone()
        };
        assert_workspace_only_launch_spec(&app, &spec, kind, &actor, "/triage");
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
                .any(|(name, _)| name == "BRAIN_TRIAGE_DONE_URL")
        );
        assert!(
            spec.environment
                .iter()
                .any(|(name, _)| name == "BRAIN_TRIAGE_TOKEN")
        );
        for forbidden in ["BRAIN_INSTANCE_ID", "BRAIN_STATE_DB", "BRAIN_RESPONSE_ID"] {
            assert!(
                spec.environment.iter().all(|(name, _)| name != forbidden),
                "ephemeral triage must omit {forbidden}"
            );
        }
        let connection = rusqlite::Connection::open(&app.db_path).expect("state database");
        let registered: i64 = connection
            .query_row("SELECT COUNT(*) FROM brain_sessions", [], |row| row.get(0))
            .expect("session count");
        assert_eq!(registered, 0, "triage sessions remain untracked");
    }
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
    app.reload_tasks().expect("reload after seeding the managed habit");

    // Press Skip on the daily-triage nudge.
    app.confirm = Some(crate::tui::ConfirmState::run_triage(
        "H35".to_owned(),
        "Morning Triage".to_owned(),
    ));
    crate::tui::handlers::run_confirm_skip(&mut app);

    // The modal is dismissed and no agent panel was launched — Skip is a pure
    // in-process CSV mutation.
    assert!(app.confirm.is_none(), "modal should be dismissed");
    assert!(app.brain.is_none(), "Skip must not launch the main brain panel");
    assert!(app.triage_brain.is_none(), "Skip must not open a triage tab");

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
    for kind in [AgentKind::Claude, AgentKind::Codex] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
        app.config.access_mode = crate::access::AccessMode::Unrestricted;
        let name = app.command_context.workspace.name().clone();
        app.command_context
            .registry_store
            .replace(&crate::workspace::MachineRegistry {
                schema_version: 2,
                default_workspace: name.clone(),
                workspaces: BTreeMap::from([(
                    name,
                    crate::workspace::WorkspaceRecord {
                        workspace_id: app.command_context.workspace.id(),
                        root: app.command_context.workspace.root().to_path_buf(),
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: serde_json::Map::from_iter([(
                            "agent_capabilities".to_owned(),
                            serde_json::json!({"mcps": "malformed"}),
                        )]),
                    },
                )]),
            })
            .expect("malformed unused machine capabilities");
        let recording = LaunchRecording::default();
        app.triage_done_url_override = Some("http://127.0.0.1:4773/triage/done".to_owned());
        app.triage_transport_override = Some(Box::new(LaunchRecordingTransport {
            recording: recording.clone(),
            alive: false,
        }));

        app.open_triage_tab();

        assert!(app.triage_brain.is_some());
        let specs = recording.0.lock().expect("launch recording");
        assert_eq!(specs.len(), 1);
        assert!(!specs[0].command.contains("--mcp-config"));
        assert!(!specs[0].command.contains("developer_instructions"));
        drop(specs);
    }
}
