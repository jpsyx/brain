use super::*;

use super::receiver_durable_support::ReceiverClock;
use crate::manual_session::{ManualSessionName, ManualSessionRole};
use crate::tui::model::SessionTabId;
use crossterm::event::{Event, KeyEvent, KeyModifiers};

mod close;
mod failure_feedback;
mod palette;
mod recurring;
mod rename;
mod restoration;

fn test_clock(app: &mut App) -> ReceiverClock {
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    clock
}

fn interactive_scope(app: &App) -> SessionScope {
    SessionScope::new(
        app.context.agent_kind(),
        app.context.workspace().id(),
        app.brain.interactive_actor().clone(),
    )
}

fn name(value: &str) -> ManualSessionName {
    ManualSessionName::parse(value, &["Brain".to_owned()]).unwrap()
}

fn active_additional(app: &App) -> SessionTabId {
    let BrainTab::Session(id) = app.effective_brain_tab() else {
        panic!("expected an additional tab")
    };
    id
}

fn ctrl_x(app: &mut App) {
    assert!(!crate::tui::event_loop::update_application(
        app,
        &Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
    ));
}

#[test]
fn a_named_manual_session_launches_fresh_in_a_selected_additional_tab_for_every_frontend() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let recording = TransportRecording::default();
        app.brain.replace_manual_transport(recording.transport());

        app.start_manual_session(name("Atlas"));

        assert_eq!(app.active_brain_tab_title(), Some("Atlas"));
        assert_eq!(recording.launch_specs().len(), 1);
        assert!(app.brain.is_manual_session_tab(app.effective_brain_tab()));
        assert_eq!(app.shell.focus(), Panel::Brain);
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.name.as_str(), "Atlas");
        assert_eq!(record.role, ManualSessionRole::Additional);
        let spec = &recording.launch_specs()[0];
        assert_eq!(
            environment_value(spec, "BRAIN_INSTANCE_ID"),
            record.id.as_str()
        );
        assert_eq!(environment_value(spec, "BRAIN_CHANNEL"), "interactive");
        assert_eq!(environment_value(spec, "BRAIN_ACTOR_ID"), "pablo");
        assert_eq!(spec.cwd, app.context.workspace().root());
        assert!(!spec.command.contains(" resume ") && !spec.command.contains("--resume"));
        assert_eq!(
            app.services
                .locked_session_for_instance(record.id.as_str(), &interactive_scope(&app)),
            Some(record.agent_session.as_str().to_owned())
        );
    }
}

#[test]
fn main_has_no_close_path_and_stays_visible_without_a_controller() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        assert!(app.brain.any_panel_visible());
        let recording = TransportRecording::default();
        app.brain.replace_brain_transport(recording.transport());
        assert!(app.open_or_focus_brain(None));
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();

        ctrl_x(&mut app);
        app.close_active_user_session();

        assert!(app.brain.main_controller().unwrap().is_alive().unwrap());
        assert_eq!(recording.shutdowns(), 0);
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap(),
            records
        );
    }
}

#[test]
fn closing_a_manual_neighbor_preserves_the_other_stable_tab_and_mapping() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let atlas = TransportRecording::default();
        app.brain.replace_manual_transport(atlas.transport());
        app.start_manual_session(name("Atlas"));
        let atlas_id = active_additional(&app);
        let beacon = TransportRecording::default();
        app.brain.replace_manual_transport(beacon.transport());
        app.start_manual_session(name("Beacon"));
        let beacon_id = active_additional(&app);
        let beacon_manual_id = app.brain.manual_session_id(beacon_id).unwrap().clone();
        app.focus_tasks();

        app.close_manual_session(atlas_id);

        assert_eq!(app.effective_brain_tab(), BrainTab::Session(beacon_id));
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(app.active_brain_tab_title(), Some("Beacon"));
        assert_eq!(atlas.shutdowns(), 1);
        assert_eq!(beacon.shutdowns(), 0);
        assert!(!app.select_brain_tab(BrainTab::Session(atlas_id)));
        let records = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, beacon_manual_id);

        ctrl_x(&mut app);

        assert_eq!(beacon.shutdowns(), 1);
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap()
                .is_empty()
        );
        assert!(app.brain.any_panel_visible());
    }
}

#[test]
fn ctrl_x_closes_skill_metadata_but_manual_close_rejects_it() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let (controller, recording) = recording_controller(&app, true, "skill");
    let id = app.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "close-skill-token",
        controller,
    );

    app.close_manual_session(id);
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(id));
    assert!(recording.events().is_empty());
    ctrl_x(&mut app);

    assert_eq!(recording.events(), [ControllerEvent::Shutdown]);
    assert_eq!(app.effective_brain_tab(), BrainTab::Main);
    assert!(app.brain.any_panel_visible());
}

#[test]
fn fresh_additional_spawn_failure_rolls_back_both_rows_without_moving_focus() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let main = TransportRecording::default();
        app.brain.replace_brain_transport(main.transport());
        assert!(app.open_or_focus_brain(None));
        app.focus_tasks();
        let before = app
            .services
            .manual_sessions(&interactive_scope(&app))
            .unwrap();
        app.brain
            .replace_manual_transport(Box::new(FailingSpawnTransport));

        app.start_manual_session(name("Atlas"));

        assert_eq!(
            app.services
                .manual_sessions(&interactive_scope(&app))
                .unwrap(),
            before
        );
        let connection = rusqlite::Connection::open(app.context.state_db_path()).unwrap();
        let rows: u32 = connection
            .query_row("SELECT count(*) FROM brain_sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert_eq!(main.shutdowns(), 0);
    }
}

#[test]
fn additional_tab_allocation_failure_releases_the_exact_lock_and_retains_the_mapping() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        crate::tui::state::exhaust_session_tab_ids(&mut app.brain);
        let recording = TransportRecording::default();
        app.brain.replace_manual_transport(recording.transport());

        app.start_manual_session(name("Atlas"));

        assert_eq!(recording.shutdowns(), 1);
        let scope = interactive_scope(&app);
        let records = app.services.manual_sessions(&scope).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            app.services
                .locked_session_for_instance(records[0].id.as_str(), &scope),
            None
        );
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
    }
}
