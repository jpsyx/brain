use super::*;
use crate::tui::overlay::Overlay;

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    assert!(!crate::tui::event_loop::update_application(
        app,
        &Event::Key(KeyEvent::new(code, modifiers)),
    ));
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

fn modal(app: &App) -> &crate::tui::modal_state::ManualSessionNameState {
    let Some(Overlay::ManualSessionName(state)) = app.overlay.as_ref() else {
        panic!("expected the naming modal to remain visible")
    };
    state
}

#[test]
fn manual_session_name_modal_keeps_blank_and_duplicate_input_visible() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.open_manual_session_name_modal();
    type_text(&mut app, "   ");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(modal(&app).error(), Some("Session name cannot be blank"));
    assert_eq!(modal(&app).buffer(), "   ");

    press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    type_text(&mut app, " brain ");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        modal(&app).error(),
        Some("A session named Brain is already open")
    );
    assert_eq!(modal(&app).buffer(), " brain ");
    assert!(app.brain.user_session_rows().is_empty());
}

#[test]
fn naming_rejects_an_open_manual_title_without_launching_another_session() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    app.open_manual_session_name_modal();
    type_text(&mut app, " atlas ");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        modal(&app).error(),
        Some("A session named Atlas is already open")
    );
    assert_eq!(app.brain.user_session_rows().len(), 1);
}

#[test]
fn escape_and_ctrl_c_cancel_manual_session_naming_without_launching() {
    for (code, modifiers) in [
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        app.open_manual_session_name_modal();
        type_text(&mut app, "Atlas");
        press(&mut app, code, modifiers);
        assert!(app.overlay.is_none());
        assert!(app.brain.user_session_rows().is_empty());
    }
}

#[test]
fn valid_enter_trims_the_name_and_launches_fresh_for_every_frontend() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        let recording = TransportRecording::default();
        app.brain.replace_manual_transport(recording.transport());
        app.open_manual_session_name_modal();
        type_text(&mut app, " Atlas ");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

        assert!(app.overlay.is_none());
        assert_eq!(app.active_brain_tab_title(), Some("Atlas"));
        assert_eq!(app.shell.focus(), Panel::Brain);
        let launches = recording.launch_specs();
        assert_eq!(launches.len(), 1);
        assert!(!launches[0].command.contains(" resume "));
        assert!(!launches[0].command.contains("--resume"));
    }
}

#[test]
fn backspace_and_ctrl_u_edit_the_single_line_and_clear_validation_errors() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.open_manual_session_name_modal();
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, "Atlás界");
    assert_eq!(modal(&app).error(), None);
    press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(modal(&app).buffer(), "Atlás");
    press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(modal(&app).buffer(), "");
}

#[test]
fn naming_is_captive_against_panel_accelerators_and_control_characters() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.open_manual_session_name_modal();
    for (code, modifiers) in [
        (KeyCode::Char('x'), KeyModifiers::CONTROL),
        (KeyCode::Char('n'), KeyModifiers::CONTROL),
        (KeyCode::Char('p'), KeyModifiers::CONTROL),
        (KeyCode::Char('b'), KeyModifiers::CONTROL),
        (KeyCode::Char('l'), KeyModifiers::ALT),
        (KeyCode::Char('s'), KeyModifiers::ALT),
        (KeyCode::Tab, KeyModifiers::NONE),
        (KeyCode::Down, KeyModifiers::NONE),
        (KeyCode::Char('\n'), KeyModifiers::NONE),
        (KeyCode::Char('\u{7}'), KeyModifiers::NONE),
    ] {
        press(&mut app, code, modifiers);
        assert_eq!(modal(&app).buffer(), "");
        assert_eq!(app.shell.focus(), Panel::Tasks);
        assert_eq!(app.shell.main_view(), crate::main_view::MainView::Tasks);
    }
    assert!(app.brain.user_session_rows().is_empty());
}

#[test]
fn naming_modal_renders_prompt_buffer_inline_error_and_footer() {
    let temporary = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.open_manual_session_name_modal();
    type_text(&mut app, "brain");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, &mut app))
        .unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    for expected in [
        "Start new brain session",
        "What would you like to name this session?",
        "> brain",
        "A session named Brain is already open",
        "Enter start  Esc cancel",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
}
