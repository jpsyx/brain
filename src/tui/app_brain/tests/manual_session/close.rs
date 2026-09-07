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
fn close_picker_lists_closeable_sessions_before_disabled_sessions() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    let (skill_controller, _) = recording_controller(&app, true, "close-picker-skill");
    app.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "close-picker-skill-token",
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
            "close-picker-receiver".to_owned(),
            receiver_controller,
        )
        .unwrap();

    app.execute_global_action(GlobalAction::CloseSession);

    let screen = rendered(&mut app);
    let atlas = screen.find("1. Atlas").unwrap();
    let triage = screen.find("2. Daily triage").unwrap();
    let main = screen.find("3. Brain").unwrap();
    let receiver = screen.find("4. Receiver · SMS").unwrap();
    assert!(screen.contains("Close a brain session"), "{screen}");
    assert!(screen.contains("[not closeable]"), "{screen}");
    assert!(
        atlas < triage && triage < main && main < receiver,
        "{screen}"
    );
}

#[test]
fn noncloseable_titles_are_struck_out_and_their_annotation_is_yellow() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.execute_global_action(GlobalAction::CloseSession);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, &mut app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (note_x, note_y) = find_text(buffer, "[not closeable]").unwrap();
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
fn selecting_closeable_rows_closes_manual_and_skill_sessions() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let manual = TransportRecording::default();
    app.brain.replace_manual_transport(manual.transport());
    app.start_manual_session(name("Atlas"));
    let (skill_controller, skill) = recording_controller(&app, true, "close-picker-skill");
    app.insert_test_skill_session(
        crate::skill_session::SkillSessionKey::DailyTriage,
        "Daily triage",
        "close-picker-skill-token",
        skill_controller,
    );

    app.execute_global_action(GlobalAction::CloseSession);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.overlay.is_none());
    assert!(app.brain.manual_session_rows().is_empty());
    assert_eq!(manual.shutdowns(), 1);
    assert_eq!(app.brain.user_session_rows().len(), 1);

    app.execute_global_action(GlobalAction::CloseSession);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.overlay.is_none());
    assert!(app.brain.user_session_rows().is_empty());
    assert_eq!(skill.events(), vec![ControllerEvent::Shutdown]);
}
