use super::*;
use crate::main_view::MainView;
use crate::tasks::view::TaskViewOptions;
use crate::tui::overlay::Overlay;
use ratatui::{Terminal, backend::TestBackend};

fn feedback_app(temporary: &tempfile::TempDir, kind: AgentKind) -> App {
    let cli = Cli::parse_from(["tasks"]);
    test_app(temporary, TaskViewOptions::from(&cli), kind)
}

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

fn choose_search_command(app: &mut App, label: &str) {
    app.shell.show_main_view(MainView::BrainSearch);
    app.focus_tasks();
    press(app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    assert!(matches!(app.overlay, Some(Overlay::SearchPalette(_))));
    type_text(app, label);
    press(app, KeyCode::Enter, KeyModifiers::NONE);
}

fn choose_close_session(app: &mut App) {
    choose_search_command(app, "Close a brain session");
    assert!(matches!(app.overlay, Some(Overlay::SessionClosePicker(_))));
    press(app, KeyCode::Enter, KeyModifiers::NONE);
}

fn start_session(app: &mut App) {
    choose_search_command(app, "Start new brain session");
    assert!(app.overlay.is_none());
    assert!(app.brain.user_session_rows().is_empty());
}

fn rendered(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, app))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(100)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_search_failure(app: &mut App, operation: &str, cause: &str) {
    assert_eq!(app.shell.main_view(), MainView::BrainSearch);
    let screen = rendered(app);
    let words = screen.split_whitespace().collect::<Vec<_>>().join(" ");
    for expected in [operation, cause, "Esc dismiss error"] {
        assert!(words.contains(expected), "missing {expected}:\n{screen}");
    }
}

#[test]
fn search_launch_failure_is_rendered_and_survives_the_next_search_key() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().unwrap();
        let mut app = feedback_app(&temporary, kind);
        app.brain
            .replace_manual_transport(Box::new(FailingSpawnTransport));

        start_session(&mut app);

        assert_search_failure(&mut app, "could not start", "injected spawn failure");
        press(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
        assert_search_failure(&mut app, "could not start", "injected spawn failure");
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!rendered(&mut app).contains("injected spawn failure"));
    }
}

#[test]
fn search_persistence_failure_survives_input_before_the_first_render() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = feedback_app(&temporary, AgentKind::Claude);
    let recording = TransportRecording::default();
    app.brain.replace_manual_transport(recording.transport());
    let connection = rusqlite::Connection::open(app.context.state_db_path()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_manual_insert BEFORE INSERT ON manual_sessions
             BEGIN SELECT RAISE(ABORT, 'injected persistence failure'); END;",
        )
        .unwrap();

    start_session(&mut app);
    press(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);

    assert_search_failure(&mut app, "could not start", "injected persistence failure");
    assert!(recording.launch_specs().is_empty());
    let rows: u32 = connection
        .query_row("SELECT count(*) FROM brain_sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn search_close_persistence_failure_is_rendered_after_the_next_key() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = feedback_app(&temporary, AgentKind::Claude);
    app.brain
        .replace_manual_transport(TransportRecording::default().transport());
    app.start_manual_session(name("Atlas"));
    let tab = active_additional(&app);
    let connection = rusqlite::Connection::open(app.context.state_db_path()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_manual_close BEFORE DELETE ON manual_sessions
             BEGIN SELECT RAISE(ABORT, 'injected close persistence failure'); END;",
        )
        .unwrap();

    choose_close_session(&mut app);

    assert_search_failure(
        &mut app,
        "session could not close",
        "injected close persistence failure",
    );
    press(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    assert_search_failure(
        &mut app,
        "session could not close",
        "injected close persistence failure",
    );
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(tab));
    assert_eq!(app.brain.user_session_rows().len(), 1);
    assert_eq!(
        app.services
            .manual_sessions(&interactive_scope(&app))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn search_close_shutdown_failure_is_rendered_and_keeps_the_session() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = feedback_app(&temporary, AgentKind::Claude);
    app.brain
        .replace_manual_transport(Box::new(FailingShutdownTransport));
    app.start_manual_session(name("Atlas"));
    let tab = active_additional(&app);

    choose_close_session(&mut app);

    assert_search_failure(
        &mut app,
        "session could not close",
        "injected shutdown failure",
    );
    press(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    assert_search_failure(
        &mut app,
        "session could not close",
        "injected shutdown failure",
    );
    assert_eq!(app.effective_brain_tab(), BrainTab::Session(tab));
    assert!(
        app.active_brain_controller_mut()
            .unwrap()
            .is_alive()
            .unwrap()
    );
}

#[test]
fn manual_failure_feedback_remains_visible_across_all_main_views() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = feedback_app(&temporary, AgentKind::Claude);
    app.brain
        .replace_manual_transport(Box::new(FailingSpawnTransport));
    start_session(&mut app);

    for (key, view) in [
        ('t', MainView::Tasks),
        ('l', MainView::BrainSearch),
        ('l', MainView::Logs),
    ] {
        press(&mut app, KeyCode::Char(key), KeyModifiers::CONTROL);
        assert_eq!(app.shell.main_view(), view);
        assert!(rendered(&mut app).contains("injected spawn failure"));
    }
}

#[test]
fn a_pending_error_does_not_steal_escape_from_the_rename_picker() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = feedback_app(&temporary, AgentKind::Claude);
    app.brain
        .replace_manual_transport(Box::new(FailingSpawnTransport));
    start_session(&mut app);

    choose_search_command(&mut app, "Rename session");
    assert!(matches!(app.overlay, Some(Overlay::SessionRenamePicker(_))));
    assert!(rendered(&mut app).contains("injected spawn failure"));
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.overlay.is_none());
    assert_search_failure(&mut app, "could not start", "injected spawn failure");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!rendered(&mut app).contains("injected spawn failure"));
}

#[test]
fn an_additional_startup_exit_reports_a_persistent_error_from_search() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = feedback_app(&temporary, AgentKind::Claude);
    let _clock = test_clock(&mut app);
    let recording = TransportRecording::default();
    app.brain.replace_manual_transport(recording.transport());
    app.start_manual_session(name("Atlas"));
    app.shell.show_main_view(MainView::BrainSearch);
    app.focus_tasks();
    recording.set_alive(false);

    app.tick_manual_sessions();
    press(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);

    assert_search_failure(&mut app, "unavailable", "exited during startup");
    assert_eq!(app.brain.user_session_rows().len(), 1);
}

struct FailingShutdownTransport;

impl AgentTransport for FailingShutdownTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn shutdown(&mut self) -> Result<(), AgentError> {
        Err(AgentError::Transport(
            "injected shutdown failure".to_owned(),
        ))
    }
}
