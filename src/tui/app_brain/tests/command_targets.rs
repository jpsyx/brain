//! The parent-set invariant end to end: a command run without its target asks
//! for one, and the same command run with a target acts on it.

use crossterm::event::{Event, KeyEvent, KeyModifiers};

use super::*;
use crate::main_view::MainView;
use crate::tui::event_loop::update_application;
use crate::tui::handlers::{handle_entry_target_picker_key, handle_task_target_picker_key};
use crate::tui::overlay::Overlay;
use crate::tui::palette::{Command, EntryCommand, TaskCommand};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    update_application(app, &Event::Key(KeyEvent::new(code, modifiers)))
}

fn app_with_tasks(temporary: &tempfile::TempDir) -> App {
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(temporary, &cli, AgentKind::Claude);
    let mut first = crate::tasks::task::test_task("T1", "not_started");
    first.name = "Draft the plan".to_owned();
    let mut second = crate::tasks::task::test_task("T2", "not_started");
    second.name = "Ship the thing".to_owned();
    app.tasks.replace_rows(vec![first, second], Vec::new());
    app
}

fn confirmed_task(app: &App) -> &str {
    let Some(Overlay::TaskConfirmation(confirm)) = app.overlay.as_ref() else {
        panic!("expected a confirmation, got another overlay");
    };
    &confirm.task_id
}

#[test]
fn a_task_command_acts_on_the_highlighted_task_in_the_tasks_view() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);
    app.shell.show_main_view(MainView::Tasks);

    app.execute_command(Command::Task(TaskCommand::MarkComplete));

    assert_eq!(confirmed_task(&app), "T1");
}

#[test]
fn a_task_command_run_from_another_view_asks_which_task_first() {
    // The palette lists every command from every view, so running a task
    // command from the brain directory must reach a task rather than act on a
    // highlight the user cannot see.
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);
    app.shell.show_main_view(MainView::BrainSearch);

    app.execute_command(Command::Task(TaskCommand::MarkComplete));

    let Some(Overlay::TaskTargetPicker(picker)) = app.overlay.as_ref() else {
        panic!("expected the task picker");
    };
    assert_eq!(picker.title(), "Mark which entry complete?");

    handle_task_target_picker_key(&mut app, &key(KeyCode::Down));
    handle_task_target_picker_key(&mut app, &key(KeyCode::Enter));

    assert_eq!(confirmed_task(&app), "T2");
}

#[test]
fn the_task_picker_can_be_filtered_by_name_before_choosing() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);
    app.shell.show_main_view(MainView::BrainSearch);
    app.execute_command(Command::Task(TaskCommand::MarkComplete));

    for character in "ship".chars() {
        handle_task_target_picker_key(&mut app, &key(KeyCode::Char(character)));
    }
    handle_task_target_picker_key(&mut app, &key(KeyCode::Enter));

    assert_eq!(confirmed_task(&app), "T2");
}

#[test]
fn escaping_the_task_picker_runs_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);
    app.shell.show_main_view(MainView::BrainSearch);
    app.execute_command(Command::Task(TaskCommand::MarkComplete));

    handle_task_target_picker_key(&mut app, &key(KeyCode::Esc));

    assert!(app.overlay.is_none());
}

#[test]
fn an_entry_command_run_from_the_tasks_view_asks_which_entry_first() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);
    let plan = app.context.workspace_root().join("projects/atlas/plan.md");
    std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
    std::fs::write(&plan, "# plan\n").unwrap();
    app.shell.show_main_view(MainView::Tasks);

    app.execute_command(Command::Entry(EntryCommand::Delete));

    let Some(Overlay::EntryTargetPicker(picker)) = app.overlay.as_ref() else {
        panic!("expected the entry picker");
    };
    assert_eq!(picker.title(), "Delete which entry?");

    handle_entry_target_picker_key(&mut app, &key(KeyCode::Enter));

    let Some(Overlay::SearchConfirmation(confirm)) = app.overlay.as_ref() else {
        panic!("choosing an entry should raise the delete confirmation");
    };
    assert!(confirm.path.starts_with(app.context.workspace_root()));
}

#[test]
fn an_entry_command_with_nothing_to_act_on_says_so_instead_of_opening_an_empty_picker() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);
    app.shell.show_main_view(MainView::Tasks);

    app.execute_command(Command::Entry(EntryCommand::Open));

    assert!(app.overlay.is_none());
    assert!(matches!(
        app.status.flash(),
        Some(crate::tui::modal_state::FlashKind::Info(message))
            if message.contains("brain directory")
    ));
}

#[test]
fn the_quit_row_leaves_the_shell_the_same_way_the_chord_does() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);

    assert!(!press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(matches!(app.overlay, Some(Overlay::CommandPalette(_))));
    for character in "quit brain".chars() {
        assert!(!press(&mut app, KeyCode::Char(character), KeyModifiers::NONE));
    }

    assert!(press(&mut app, KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn the_shortcuts_row_opens_the_same_help_modal_alt_s_does() {
    let temporary = tempfile::tempdir().unwrap();
    let mut app = app_with_tasks(&temporary);

    press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    for character in "keyboard shortcuts".chars() {
        press(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(matches!(app.overlay, Some(Overlay::Help(_))));
}
