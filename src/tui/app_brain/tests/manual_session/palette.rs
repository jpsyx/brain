use super::*;
use crate::main_view::MainView;
use crate::skill_session::SkillSessionKey;
use crate::tui::action::GlobalAction;
use crate::tui::overlay::{Overlay, close_overlay};

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    assert!(!crate::tui::event_loop::update_application(
        app,
        &Event::Key(KeyEvent::new(code, modifiers))
    ));
}

fn open_palette(app: &mut App, view: MainView) {
    close_overlay(&mut app.overlay);
    app.shell.show_main_view(view);
    app.focus_tasks();
    key(app, KeyCode::Char('p'), KeyModifiers::CONTROL);
}

fn palette_rows(app: &App) -> Vec<(String, GlobalAction, Option<&'static str>)> {
    match app.overlay.as_ref().unwrap() {
        Overlay::TaskPalette(palette) => palette
            .rows()
            .iter()
            .filter_map(|row| match row.action {
                crate::tui::palette::TaskAction::Global(action) => {
                    Some((row.label.clone(), action, row.shortcut))
                }
                _ => None,
            })
            .collect(),
        Overlay::SearchPalette(palette) => palette
            .rows()
            .iter()
            .filter_map(|row| match row.action {
                crate::menu::SearchAction::Global(action) => {
                    Some((row.label.clone(), action, row.shortcut))
                }
                _ => None,
            })
            .collect(),
        _ => panic!("expected a command palette"),
    }
}

fn choose(app: &mut App, label: &str) {
    for character in label.chars() {
        key(app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    key(app, KeyCode::Enter, KeyModifiers::NONE);
}

#[test]
fn runtime_palettes_receive_identical_user_tabs_and_exclude_receiver_rows() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_brain_transport(TransportRecording::default().transport());
    assert!(app.open_or_focus_brain(None));
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    let atlas = active_additional(&app);
    let (controller, _) = recording_controller(&app, true, "skill");
    let triage = app.insert_test_skill_session(
        SkillSessionKey::DailyTriage,
        "Daily triage",
        "palette-skill-token",
        controller,
    );
    let receiver = TransportRecording::default();
    let controller = AgentController::configured(
        app.context.command(),
        app.context.agent_kind(),
        crate::actor::test_actor("receiver"),
        receiver.transport(),
    );
    let receiver_id = app
        .brain
        .add_receiver_run(
            crate::state::ReceiverJobId::from(uuid::Uuid::new_v4()),
            "Receiver · SMS".into(),
            "palette-receiver".into(),
            controller,
        )
        .unwrap();
    for view in [MainView::Tasks, MainView::BrainSearch] {
        open_palette(&mut app, view);
        let rows = palette_rows(&app);
        for (label, action) in [
            ("Start new brain session", GlobalAction::StartManualSession),
            ("Rename session", GlobalAction::RenameSession),
            (
                "Show main brain session",
                GlobalAction::ShowMainBrainSession,
            ),
            ("Show Atlas session", GlobalAction::ShowSessionTab(atlas)),
            ("Close Atlas session", GlobalAction::CloseSessionTab(atlas)),
            (
                "Show Daily triage session",
                GlobalAction::ShowSessionTab(triage),
            ),
            (
                "Close Daily triage session",
                GlobalAction::CloseSessionTab(triage),
            ),
        ] {
            assert!(rows.contains(&(label.to_owned(), action, None)), "{rows:?}");
        }
        assert!(
            rows.iter()
                .all(|(label, _, _)| !label.contains("Receiver · SMS")
                    && label != "Close Brain session"
                    && label != "Close brain")
        );
        assert!(
            rows.iter()
                .any(|(_, action, _)| *action == GlobalAction::MessageBrain)
        );
        assert!(
            !rows.iter().any(|(_, action, _)| *action
                == GlobalAction::RunSkillSession(SkillSessionKey::DailyTriage))
        );
    }
    close_overlay(&mut app.overlay);
    app.execute_global_action(GlobalAction::CloseSessionTab(receiver_id));
    assert_eq!(receiver.shutdowns(), 0);
    assert!(app.brain.receiver_run_controller(receiver_id).is_some());
}

#[test]
fn both_palette_start_rows_launch_a_workspace_named_session_without_a_modal() {
    for view in [MainView::Tasks, MainView::BrainSearch] {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        app.brain
            .replace_brain_transport(TransportRecording::default().transport());
        assert!(app.open_or_focus_brain(None));
        app.brain
            .replace_manual_transport(TransportRecording::default().transport());
        app.start_manual_session(name("Atlas"));
        open_palette(&mut app, view);
        choose(&mut app, "Message brain");
        assert_eq!(app.effective_brain_tab(), BrainTab::Main);
        assert!(app.overlay.is_none());
        open_palette(&mut app, view);
        app.brain
            .replace_manual_transport(TransportRecording::default().transport());
        choose(&mut app, "Start new brain session");
        assert!(app.overlay.is_none());
        let title = app.active_brain_tab_title().unwrap();
        let suffix = title.strip_prefix("family-").unwrap();
        assert_eq!(suffix.len(), 3);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_lowercase()));
    }
}

#[test]
fn stable_palette_close_and_show_actions_survive_a_neighbor_closing() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    let atlas = active_additional(&app);
    let (controller, recording) = recording_controller(&app, true, "skill");
    let skill = app.insert_test_skill_session(
        SkillSessionKey::DailyTriage,
        "Daily triage",
        "stable-palette-skill",
        controller,
    );
    app.execute_global_action(GlobalAction::CloseSessionTab(atlas));
    app.execute_global_action(GlobalAction::ShowSessionTab(skill));
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(skill));
    assert_eq!(app.active_brain_tab_title(), Some("Daily triage"));
    app.execute_global_action(GlobalAction::CloseSessionTab(skill));
    assert_eq!(recording.events(), [ControllerEvent::Shutdown]);
    assert_eq!(app.effective_brain_tab(), BrainTab::Main);
    app.execute_global_action(GlobalAction::CloseSessionTab(atlas));
    assert!(app.brain.user_session_rows().is_empty());
}
