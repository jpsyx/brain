use super::*;
use crate::tui::action::GlobalAction;

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    assert!(!crate::tui::event_loop::update_application(
        app,
        &Event::Key(KeyEvent::new(code, modifiers)),
    ));
}

fn rendered(app: &mut App) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, app))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

fn find_text(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
    let needle: Vec<char> = needle.chars().collect();
    for y in 0..buffer.area.height {
        for x in 0..=buffer
            .area
            .width
            .saturating_sub(u16::try_from(needle.len()).unwrap())
        {
            if needle.iter().enumerate().all(|(offset, expected)| {
                buffer[(x + u16::try_from(offset).unwrap(), y)].symbol() == expected.to_string()
            }) {
                return Some((x, y));
            }
        }
    }
    None
}

#[test]
fn rename_picker_lists_every_session_and_marks_only_additional_manual_tabs_renameable() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    let (skill_controller, _) = recording_controller(&app, true, "skill");
    app.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "rename-picker-skill",
        skill_controller,
    );
    let receiver_controller = AgentController::configured(
        app.context.command(),
        app.context.agent_kind(),
        crate::actor::test_actor("receiver"),
        TransportRecording::default().transport(),
    );
    app.brain
        .add_receiver_run(
            crate::state::ReceiverJobId::from(uuid::Uuid::new_v4()),
            "Receiver · SMS".to_owned(),
            "rename-picker-receiver".to_owned(),
            receiver_controller,
        )
        .unwrap();

    app.execute_global_action(GlobalAction::RenameSession);

    assert!(app.overlay.is_some());
    let rendered = rendered(&mut app);
    for expected in [
        "Rename session",
        "1. Atlas",
        "2. Brain",
        "3. Daily triage",
        "4. Receiver · SMS",
        "[not renameable]",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
}

#[test]
fn nonrenameable_titles_are_struck_out_and_their_annotation_is_yellow() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.execute_global_action(GlobalAction::RenameSession);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, &mut app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (note_x, note_y) = find_text(buffer, "[not renameable]").unwrap();
    let brain_x = (0..note_x)
        .find(|x| {
            ['B', 'r', 'a', 'i', 'n']
                .iter()
                .enumerate()
                .all(|(offset, expected)| {
                    buffer[(*x + u16::try_from(offset).unwrap(), note_y)].symbol()
                        == expected.to_string()
                })
        })
        .unwrap();

    assert!(
        buffer[(brain_x, note_y)]
            .modifier
            .contains(ratatui::style::Modifier::CROSSED_OUT)
    );
    assert_eq!(buffer[(note_x, note_y)].fg, crate::render::ACCENT_YELLOW);
}

#[test]
fn selecting_an_additional_manual_session_opens_a_prefilled_rename_input() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    app.execute_global_action(GlobalAction::RenameSession);

    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    let rendered = rendered(&mut app);
    assert!(rendered.contains("Rename Atlas session"), "{rendered}");
    assert!(rendered.contains("> Atlas"), "{rendered}");
}

#[test]
fn submitting_a_new_title_renames_the_tab_and_mapping_without_touching_the_native_session() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let recording = TransportRecording::default();
    app.brain.replace_manual_transport(recording.transport());
    app.start_manual_session(name("Atlas"));
    let before = app
        .services
        .manual_sessions(&interactive_scope(&app))
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    app.execute_global_action(GlobalAction::RenameSession);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    for character in "Beacon".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }

    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.overlay.is_none());
    assert_eq!(app.active_brain_tab_title(), Some("Beacon"));
    let after = app
        .services
        .manual_sessions(&interactive_scope(&app))
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.agent_session, before.agent_session);
    assert_eq!(after.name.as_str(), "Beacon");
    assert_eq!(recording.launch_specs().len(), 1);
}

#[test]
fn main_and_receiver_rows_do_not_open_the_rename_input() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let receiver_controller = AgentController::configured(
        app.context.command(),
        app.context.agent_kind(),
        crate::actor::test_actor("receiver"),
        TransportRecording::default().transport(),
    );
    app.brain
        .add_receiver_run(
            crate::state::ReceiverJobId::from(uuid::Uuid::new_v4()),
            "Receiver · SMS".to_owned(),
            "nonrenameable-receiver".to_owned(),
            receiver_controller,
        )
        .unwrap();
    app.execute_global_action(GlobalAction::RenameSession);

    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(
        app.overlay,
        Some(crate::tui::overlay::Overlay::SessionRenamePicker(_))
    ));
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(
        app.overlay,
        Some(crate::tui::overlay::Overlay::SessionRenamePicker(_))
    ));
}

#[test]
fn rename_validation_keeps_blank_and_duplicate_titles_in_the_input() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Beacon"));
    app.execute_global_action(GlobalAction::RenameSession);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(rendered(&mut app).contains("Session name cannot be blank"));
    for character in " beacon ".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(rendered(&mut app).contains("A session named Beacon is already open"));
    assert_eq!(
        app.brain
            .manual_session_rows()
            .into_iter()
            .map(|row| row.title)
            .collect::<Vec<_>>(),
        ["Atlas".to_owned(), "Beacon".to_owned()]
    );
}
