use super::*;

#[test]
fn open_triage_tab_launches_the_selected_ephemeral_untracked_controller() {
    let cli = Cli::parse_from(["tasks"]);

    for kind in [AgentKind::Claude, AgentKind::Codex] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut app = test_app(&temporary, &cli, kind);
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
        assert!(spec.command.contains("'/triage'"));
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
