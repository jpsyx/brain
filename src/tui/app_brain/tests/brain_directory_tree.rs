//! The brain-directory tree's app-level glue: the cursor-free explorer, and
//! the hidden-files toggle, which has to write portable config as well as the
//! live field the tree reads.

use super::*;
use crate::main_view::MainView;
use crate::tui::state::BrainDirView;

/// A workspace with one visible note plus a dotted directory holding a note
/// whose own name looks ordinary — the pair the hidden rule has to agree on.
fn app_with_brain_files(temporary: &tempfile::TempDir) -> App {
    let cli = Cli::parse_from(["tasks"]);
    let app = test_app(temporary, &cli, AgentKind::Claude);
    let root = app.context.workspace_root().to_path_buf();
    std::fs::create_dir_all(root.join("projects/.obsidian")).expect("create the dotted directory");
    std::fs::write(root.join("projects/plan.md"), "# plan").expect("write the note");
    std::fs::write(root.join("projects/.obsidian/notes.md"), "x").expect("write the hidden note");
    app
}

fn saved_show_hidden_files(app: &App) -> bool {
    crate::config::Config::load(app.context.workspace()).show_hidden_files
}

#[test]
fn the_explorer_opens_the_tree_at_the_brain_root_from_a_fresh_walk() {
    // `Ctrl+E` carries nothing over: no scope and no cursor. The picker in
    // this shell holds no entries at all, so a row on screen proves the
    // explorer walked the disk for itself.
    let temporary = tempfile::tempdir().expect("temp dir");
    let mut app = app_with_brain_files(&temporary);
    let root = app.context.workspace_root().to_path_buf();

    app.open_file_explorer();

    assert_eq!(app.shell.main_view(), MainView::BrainSearch);
    assert_eq!(app.shell.brain_dir_view(), BrainDirView::Tree);
    assert_eq!(app.shell.tree_identifiers(), vec![root.join("projects")]);
    assert_eq!(
        app.shell.selected_tree_path(),
        Some(root.join("projects")),
        "the first row is selected, so the tree has a cursor to move"
    );
}

#[test]
fn toggling_hidden_files_writes_the_live_field_and_the_config_together() {
    // The CLI-palette parity rule: silencing or showing something from the
    // palette is the same decision as `brain config set`, so it survives a
    // restart rather than being a session-only choice.
    let temporary = tempfile::tempdir().expect("temp dir");
    let mut app = app_with_brain_files(&temporary);
    assert!(!app.shell.tree_show_hidden());
    assert!(!saved_show_hidden_files(&app));

    app.toggle_hidden_files();

    assert!(app.shell.tree_show_hidden());
    assert!(saved_show_hidden_files(&app), "the flip has to persist");

    app.toggle_hidden_files();

    assert!(!app.shell.tree_show_hidden());
    assert!(!saved_show_hidden_files(&app));
}
