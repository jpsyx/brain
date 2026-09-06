use super::*;

use crate::manual_session::{ManualSessionId, ManualSessionRecord};

pub(super) fn saved_record(
    id: &str,
    native: &str,
    title: &str,
    position: u32,
) -> ManualSessionRecord {
    let id = ManualSessionId::parse(id).unwrap();
    let native = AgentSession::new(native).unwrap();
    if position == 0 {
        ManualSessionRecord::main(id, native)
    } else {
        ManualSessionRecord::additional(id, native, name(title), position)
    }
}

pub(super) fn persist(app: &App, record: &ManualSessionRecord) {
    app.services
        .register_fresh_manual_session(record, 42, &interactive_scope(app))
        .unwrap();
    app.services
        .release_manual_session(&record.id, &interactive_scope(app))
        .unwrap();
}

#[test]
fn additional_restoration_async_startup_failure_preserves_its_saved_record() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let prior = test_app(&temporary, &cli, kind);
        persist(&prior, &saved_record("main", "missing-main", "Brain", 0));
        persist(&prior, &saved_record("atlas", "missing-atlas", "Atlas", 1));
        let mut app = test_app(&temporary, &cli, kind);
        let clock = test_clock(&mut app);
        let main = TransportRecording::default();
        let failed = TransportRecording::default();
        app.brain.replace_brain_transport(main.transport());
        app.brain.replace_manual_transport(failed.transport());
        app.restore_manual_sessions();
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        let tabs = app.brain.session_tab_ids();
        failed.set_alive(false);

        app.tick_manual_sessions();
        clock.advance(std::time::Duration::from_secs(60));
        app.tick_manual_sessions();

        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap(),
            records
        );
        assert_eq!(app.brain.session_tab_ids(), tabs);
        assert_eq!(app.brain.tab_titles(), ["Brain", "Atlas"]);
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(failed.shutdowns(), 1);
        assert_eq!(
            app.services
                .locked_session_for_instance("atlas", &interactive_scope(&app)),
            None
        );
    }
}

#[test]
fn startup_restores_main_first_then_additional_position_order_for_every_frontend() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let prior = test_app(&temporary, &cli, kind);
        let records = [
            saved_record("beacon", "missing-beacon", "Beacon", 2),
            saved_record("saved-main", "missing-main", "Brain", 0),
            saved_record("atlas", "missing-atlas", "Atlas", 1),
        ];
        for record in &records {
            persist(&prior, record);
        }
        drop(prior);
        let mut restored = test_app(&temporary, &cli, kind);
        assert_eq!(restored.brain.instance(), "saved-main");
        let recording = TransportRecording::default();
        restored
            .brain
            .replace_brain_transport(recording.transport());
        restored
            .brain
            .replace_manual_transport(recording.transport());
        restored
            .brain
            .replace_manual_transport(recording.transport());

        restored.restore_manual_sessions();

        assert_eq!(restored.brain.tab_titles(), ["Brain", "Atlas", "Beacon"]);
        let specs = recording.launch_specs();
        assert_eq!(
            specs
                .iter()
                .map(|spec| environment_value(spec, "BRAIN_INSTANCE_ID"))
                .collect::<Vec<_>>(),
            ["saved-main", "atlas", "beacon"]
        );
        let current = restored
            .services
            .manual_sessions(&interactive_scope(&restored))
            .unwrap();
        assert_eq!(
            current
                .iter()
                .map(|row| (row.name.as_str(), row.position))
                .collect::<Vec<_>>(),
            [("Brain", 0), ("Atlas", 1), ("Beacon", 2)]
        );
        for record in &current {
            assert!(
                !records
                    .iter()
                    .any(|prior| prior.agent_session == record.agent_session)
            );
        }
        assert_eq!(restored.shell.focus(), Panel::Tasks);
        assert_eq!(restored.effective_brain_tab(), BrainTab::Main);

        restored.restore_manual_sessions();
        assert_eq!(
            recording.launch_specs().len(),
            3,
            "restoration is consumed once"
        );
    }
}

#[test]
fn an_additional_restore_failure_keeps_its_durable_record_and_current_focus() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let prior = test_app(&temporary, &cli, kind);
        persist(
            &prior,
            &saved_record("saved-main", "missing-main", "Brain", 0),
        );
        persist(&prior, &saved_record("atlas", "missing-atlas", "Atlas", 1));
        let mut restored = test_app(&temporary, &cli, kind);
        let recording = TransportRecording::default();
        restored
            .brain
            .replace_brain_transport(recording.transport());
        restored
            .brain
            .replace_manual_transport(Box::new(FailingSpawnTransport));

        restored.restore_manual_sessions();

        let records = restored
            .services
            .manual_sessions(&interactive_scope(&restored))
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].id.as_str(), "atlas");
        assert_eq!(records[1].name.as_str(), "Atlas");
        assert_eq!(records[1].position, 1);
        assert!(restored.brain.session_tab_ids().is_empty());
        assert_eq!(restored.shell.focus(), Panel::Tasks);
        assert_eq!(restored.effective_brain_tab(), BrainTab::Main);
        assert_eq!(
            restored
                .services
                .locked_session_for_instance("atlas", &interactive_scope(&restored)),
            None
        );
    }
}

#[test]
fn shell_shutdown_releases_every_exact_manual_lock_and_preserves_all_mappings() {
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
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        let foreign_scope = SessionScope::new(kind, app.context.workspace().id(), sms_actor());
        let foreign = AgentSession::new("foreign-native").unwrap();
        SessionStore::register(
            &app.services,
            &foreign,
            app.brain.instance(),
            42,
            &foreign_scope,
        )
        .unwrap();

        assert!(app.shutdown_agent_controllers().is_empty());
        app.release_manual_session_locks().unwrap();

        assert_eq!(main.shutdowns(), 1);
        assert_eq!(atlas.shutdowns(), 1);
        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap(),
            records
        );
        for record in &records {
            assert_eq!(
                app.services
                    .locked_session_for_instance(record.id.as_str(), &interactive_scope(&app)),
                None
            );
        }
        assert_eq!(
            app.services
                .locked_session_for_instance(app.brain.instance(), &foreign_scope),
            Some("foreign-native".to_owned())
        );
    }
}

#[test]
fn restored_manual_sessions_resume_only_their_exact_native_id_for_every_frontend() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let prior = test_app(&temporary, &cli, kind);
        persist(
            &prior,
            &saved_record("saved-main", "missing-main", "Brain", 0),
        );
        persist(&prior, &saved_record("atlas", "session-1", "Atlas", 1));
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
        let mut restored = test_app(&temporary, &cli, kind);
        let main = TransportRecording::default();
        let atlas = TransportRecording::default();
        restored.brain.replace_brain_transport(main.transport());
        restored.brain.replace_manual_transport(atlas.transport());

        restored.restore_manual_sessions();

        let specs = atlas.launch_specs();
        assert_eq!(specs.len(), 1);
        let flag = match kind {
            AgentKind::Claude => "--resume",
            AgentKind::Codex => "resume",
            AgentKind::OpenCode => "--session",
        };
        assert!(
            specs[0].command.contains(&format!("{flag} 'session-1'")),
            "{}",
            specs[0].command
        );
        assert!(!main.launch_specs()[0].command.contains("'session-1'"));
        let records = restored
            .services
            .manual_sessions(&interactive_scope(&restored))
            .unwrap();
        assert_eq!(records[1].agent_session.as_str(), "session-1");
    }
}
