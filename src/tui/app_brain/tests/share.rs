use super::*;
use crate::main_view::MainView;
use crate::tui::overlay::Overlay;
use crate::tui::palette::{Command, EntryCommand, catalog_rows};
use crate::tui::share::{SharePhase, handle_key};
use crossterm::event::{KeyEvent, KeyModifiers};

fn app(tmp: &tempfile::TempDir) -> App {
    let cli = Cli::parse_from(["tasks"]);
    test_app(tmp, &cli, AgentKind::Claude)
}
fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        handle_key(
            app,
            &KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
        );
    }
}
fn state(app: &App) -> &crate::tui::share::ShareState {
    let Some(Overlay::Share(state)) = &app.overlay else {
        panic!("share modal missing")
    };
    state
}

#[test]
fn sharing_is_listed_everywhere_and_task_view_prompts_for_a_path() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app(&tmp);
    app.shell.show_main_view(MainView::Tasks);
    let context = app.palette_context();
    assert!(
        catalog_rows(&context)
            .iter()
            .any(|row| row.action == Command::Entry(EntryCommand::Share))
    );
    app.execute_command(Command::Entry(EntryCommand::Share));
    assert!(matches!(state(&app).phase, SharePhase::Path));
    type_text(&mut app, "/not/a/real/file.pdf");
    app.submit_share();
    assert!(matches!(state(&app).phase, SharePhase::Path));
    assert!(
        state(&app)
            .error
            .as_ref()
            .unwrap()
            .contains("not a valid file or directory")
    );
}

#[test]
fn picked_file_prompts_for_expiration_and_invalid_duration_stays_editable() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app(&tmp);
    let path = app.context.workspace_root().join("resources/paper.pdf");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"PDF").unwrap();
    app.run_entry_command(EntryCommand::Share, &path);
    assert!(matches!(&state(&app).phase, SharePhase::Expiration(picked) if picked == &path));
    assert_eq!(state(&app).duration().unwrap().as_secs(), 604_800);
    type_text(&mut app, "1 year");
    app.submit_share();
    assert!(state(&app).error.as_ref().unwrap().contains("7 days"));
    assert_eq!(state(&app).buffer, "1 year");
    handle_key(&mut app, &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
}

#[test]
fn highlighted_search_and_explorer_targets_bypass_the_path_question() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app(&tmp);
    let root = app.context.workspace_root().to_path_buf();
    std::fs::create_dir_all(root.join("resources")).unwrap();
    std::fs::write(root.join("resources/paper.pdf"), b"PDF").unwrap();
    app.search_refresh();
    app.shell.show_main_view(MainView::BrainSearch);
    app.execute_command(Command::Entry(EntryCommand::Share));
    assert!(matches!(state(&app).phase, SharePhase::Expiration(_)));
    crate::tui::overlay::close_overlay(&mut app.overlay);
    app.open_file_explorer();
    let expected = app.shell.selected_tree_path().unwrap();
    app.execute_command(Command::Entry(EntryCommand::Share));
    assert!(matches!(&state(&app).phase, SharePhase::Expiration(path) if path == &expected));
}

#[test]
fn submitted_path_advances_to_blank_expiration_and_modal_renders_default() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app(&tmp);
    let path = app.context.workspace_root().join("paper.pdf");
    std::fs::write(&path, b"PDF").unwrap();
    app.shell.show_main_view(MainView::Tasks);
    app.execute_command(Command::Entry(EntryCommand::Share));
    type_text(&mut app, path.to_str().unwrap());
    app.submit_share();
    assert!(matches!(state(&app).phase, SharePhase::Expiration(_)));
    assert!(state(&app).buffer.is_empty());
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, &mut app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.contains("7 days"));
    assert!(text.contains("paper.pdf"));
}
