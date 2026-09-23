//! The capture-note command end to end: the input modal it raises, the file it
//! writes into `capture/`, and the path it hands back to be opened.

use super::*;
use crate::tui::action::GlobalAction;
use crate::tui::modal_state::FlashKind;
use crate::tui::overlay::Overlay;
use crate::tui::palette::Command;

fn app(temporary: &tempfile::TempDir) -> App {
    let cli = Cli::parse_from(["tasks"]);
    test_app(temporary, &cli, AgentKind::Claude)
}

/// The whole terminal as one string, so a single-line assertion can look for
/// text the modal drew.
fn rendered(app: &mut App) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
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

fn typed(app: &mut App, text: &str) {
    let Some(Overlay::CaptureNote(state)) = app.overlay.as_mut() else {
        panic!("expected the capture-note modal");
    };
    for character in text.chars() {
        state.push(character);
    }
}

#[test]
fn the_command_raises_an_input_modal_whose_hint_names_the_default_timestamp() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app(&temporary);

    app.execute_command(Command::Global(GlobalAction::CreateCaptureNote));

    let Some(Overlay::CaptureNote(state)) = app.overlay.as_ref() else {
        panic!("expected the capture-note modal");
    };
    assert_eq!(
        state.helper_text(),
        format!(
            "If left empty the note's title will default to the timestamp {}",
            state.timestamp()
        )
    );
    assert!(state.buffer().is_empty());
}

#[test]
fn submitting_a_name_writes_the_kebab_cased_note_and_closes_the_modal() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app(&temporary);
    let root = app.context.workspace_root().to_path_buf();
    app.execute_command(Command::Global(GlobalAction::CreateCaptureNote));
    typed(&mut app, "My Note Name");

    let path = app.submit_capture_note().expect("a created note");

    assert_eq!(path, root.join("capture").join("my-note-name.md"));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the note"),
        "# My Note Name\n\n"
    );
    assert!(
        app.overlay.is_none(),
        "the modal closes once the note exists"
    );
    assert!(matches!(app.status.flash(), Some(FlashKind::Info(_))));
}

#[test]
fn submitting_nothing_writes_the_note_the_hint_promised() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app(&temporary);
    let root = app.context.workspace_root().to_path_buf();
    app.execute_command(Command::Global(GlobalAction::CreateCaptureNote));
    let Some(Overlay::CaptureNote(state)) = app.overlay.as_ref() else {
        panic!("expected the capture-note modal");
    };
    let timestamp = state.timestamp().to_owned();

    let path = app.submit_capture_note().expect("a created note");

    assert_eq!(
        path,
        root.join("capture").join(format!(
            "{}.md",
            crate::capture_note::kebab_case(&timestamp)
        ))
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the note"),
        format!("# {timestamp}\n\n")
    );
}

#[test]
fn a_new_note_is_searchable_in_the_brain_directory_without_a_manual_refresh() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app(&temporary);
    app.execute_command(Command::Global(GlobalAction::CreateCaptureNote));
    typed(&mut app, "Fresh Idea");

    let path = app.submit_capture_note().expect("a created note");

    assert!(
        app.shell
            .search_entries()
            .iter()
            .any(|entry| entry.path == path),
        "the new note should already be in the brain-directory entry set"
    );
}

#[test]
fn the_modal_puts_the_whole_hint_on_screen() {
    // The hint is the only place the timestamp format is stated, so a modal
    // too narrow to hold it would silently drop the promise it makes.
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app(&temporary);
    app.execute_command(Command::Global(GlobalAction::CreateCaptureNote));
    let Some(Overlay::CaptureNote(state)) = app.overlay.as_ref() else {
        panic!("expected the capture-note modal");
    };
    let hint = state.helper_text();

    let screen = rendered(&mut app);

    assert!(screen.contains("New capture note"), "{screen}");
    assert!(screen.contains(&hint), "the hint was cut off: {screen}");
}
