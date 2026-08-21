//! The brain-directory (fuzzy search) main view's key handling and actions.
//!
//! Ported from the pre-merge standalone brain shell. Drives the embedded
//! `picker::App` (`app.search`) — its query, navigation, and in-place file
//! opening. Search palette and confirmation data live in the shell's single
//! overlay slot. Only invoked while `main_view == MainView::BrainSearch`
//! and the main panel is focused; the app-level chords (view switching,
//! brain-panel open/close/new, `Alt+S` help, `Ctrl+Q` quit) are intercepted
//! upstream in `event_loop` and never reach here.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Overlay, close_overlay, open_overlay, replace_overlay};
use crate::entry::{self, Bucket};
use crate::main_view::MainView;
use crate::menu::{self, Choice};
use crate::open_target;
use crate::{confirm, picker};

impl App {
    /// Re-walk `roots` into the search picker, clearing the query (a scope
    /// switch from the brain-search palette).
    pub(crate) fn search_rescope(&mut self, roots: &[(Bucket, std::path::PathBuf)]) {
        if let Ok(entries) = entry::collect(&self.brain_root, roots) {
            self.search.set_entries(&entries);
        }
    }

    /// Re-walk the full bucket set into the search picker, keeping the query
    /// (`Ctrl-R`, or after a PDF is created / an entry is trashed).
    pub(crate) fn search_refresh(&mut self) {
        if let Ok(entries) = entry::collect(&self.brain_root, &all_bucket_roots(&self.brain_root)) {
            self.search.reload_entries(&entries);
        }
    }
}

pub(crate) fn all_bucket_roots(brain_root: &Path) -> Vec<(Bucket, std::path::PathBuf)> {
    vec![
        (Bucket::Projects, brain_root.join("projects")),
        (Bucket::Areas, brain_root.join("areas")),
        (Bucket::Resources, brain_root.join("resources")),
        (Bucket::Archive, brain_root.join("archive")),
    ]
}

fn single_bucket_root(brain_root: &Path, bucket: Bucket) -> Vec<(Bucket, std::path::PathBuf)> {
    let dir = match bucket {
        Bucket::Projects => "projects",
        Bucket::Areas => "areas",
        Bucket::Resources => "resources",
        Bucket::Archive => "archive",
    };
    vec![(bucket, brain_root.join(dir))]
}

/// Handle a keystroke while the brain-search view has focus. Returns `true`
/// when the shell should quit (Esc / Ctrl+C from the picker).
pub(crate) fn handle_search_view_key(app: &mut App, k: &KeyEvent, ctrl: bool, alt: bool) -> bool {
    match k.code {
        KeyCode::Esc => return true,
        KeyCode::Char('c') if ctrl => return true,

        // Enter opens the selection in place (shell stays up); Ctrl-Enter
        // reveals it in Finder.
        KeyCode::Enter => {
            if let Some(path) = app.search.selected_path() {
                if ctrl {
                    reveal_in_finder(&path);
                } else {
                    open_selection(&path);
                }
            }
        }

        // Ctrl-P opens the brain-search command palette. "Message brain" is
        // offered only when the panel is closed.
        KeyCode::Char('p') if ctrl => {
            app.refresh_receiver_enabled();
            let palette = app.search.search_palette(
                app.panel_side,
                app.brain.is_none(),
                app.receiver_enabled,
            );
            open_overlay(&mut app.overlay, Overlay::SearchPalette(palette));
        }
        // Ctrl-G: "Create PDF" confirmation for a highlighted markdown file.
        KeyCode::Char('g') if ctrl => {
            if let Some(path) = app.search.selected_markdown_path() {
                let confirm = picker::App::pdf_confirmation(path);
                open_overlay(&mut app.overlay, Overlay::SearchConfirmation(confirm));
            }
        }
        // Ctrl-R: re-walk the scope, keeping the query.
        KeyCode::Char('r') if ctrl => app.search_refresh(),
        // Ctrl-D: red "Delete" confirmation for the highlighted entry.
        KeyCode::Char('d') if ctrl => {
            if let Some(path) = app.search.selected_path() {
                let confirm = picker::App::delete_confirmation(path);
                open_overlay(&mut app.overlay, Overlay::SearchConfirmation(confirm));
            }
        }

        KeyCode::Up => app.search.move_up(),
        KeyCode::Char('k') if ctrl => app.search.move_up(),
        KeyCode::Down => app.search.move_down(),
        KeyCode::Char('j') if ctrl => app.search.move_down(),
        KeyCode::PageUp => app.search.page_up(),
        KeyCode::PageDown => app.search.page_down(),
        KeyCode::Home => app.search.jump_first(),
        KeyCode::End => app.search.jump_last(),

        KeyCode::Backspace => app.search.pop_query(),
        KeyCode::Char('u') if ctrl => app.search.clear_query(),
        KeyCode::Char('w') if ctrl => app.search.delete_word(),

        KeyCode::Char(c) if !ctrl && !alt => app.search.push_query(c),
        _ => {}
    }
    false
}

pub(crate) fn route_search_palette(app: &mut App, k: &KeyEvent) {
    let Some(Overlay::SearchPalette(palette)) = app.overlay.as_mut() else {
        return;
    };
    match menu::handle_key(palette, *k) {
        menu::Step::Continue => {}
        menu::Step::Cancel => {
            close_overlay(&mut app.overlay);
        }
        menu::Step::Confirm(choice) => {
            if choice == Choice::Delete {
                if let Some(path) = app.search.selected_path() {
                    let confirm = picker::App::delete_confirmation(path);
                    replace_overlay(&mut app.overlay, Overlay::SearchConfirmation(confirm));
                } else {
                    close_overlay(&mut app.overlay);
                }
                return;
            }
            close_overlay(&mut app.overlay);
            dispatch_choice(app, choice);
        }
    }
}

pub(crate) fn route_search_confirm(app: &mut App, k: &KeyEvent) {
    let step = match app.overlay.as_mut() {
        Some(Overlay::SearchConfirmation(confirm)) => confirm::handle_key(confirm, *k),
        Some(
            Overlay::TaskPalette(_)
            | Overlay::BrainInput(_)
            | Overlay::TaskConfirmation(_)
            | Overlay::SearchPalette(_)
            | Overlay::LinkPicker(_)
            | Overlay::AssigneeFilter(_)
            | Overlay::Help(_)
            | Overlay::SyncLog(_),
        )
        | None => return,
    };
    match step {
        confirm::Step::Continue => {}
        confirm::Step::Cancel => {
            close_overlay(&mut app.overlay);
        }
        confirm::Step::Accept => {
            if let Some(Overlay::SearchConfirmation(confirm)) = close_overlay(&mut app.overlay) {
                match confirm.kind {
                    confirm::ConfirmKind::Pdf => create_pdf_inline(app, &confirm.path),
                    confirm::ConfirmKind::Delete => {
                        let _ = open_target::move_to_trash(&confirm.path);
                    }
                }
                app.search_refresh();
            }
        }
    }
}

fn dispatch_choice(app: &mut App, choice: Choice) {
    match choice {
        Choice::CreatePdf => {
            if let Some(path) = app.search.selected_markdown_path() {
                create_pdf_inline(app, &path);
                app.search_refresh();
            }
        }
        Choice::OpenFile => {
            if let Some(path) = app.search.selected_path() {
                open_selection(&path);
            }
        }
        Choice::OpenDir => {
            if let Some(path) = app.search.selected_path() {
                reveal_in_finder(&path);
            }
        }
        Choice::Delete => {}
        // Open (or focus) the app-level brain panel.
        Choice::Msg => {
            app.open_or_focus_brain(None);
        }
        // Switch main view instead of the old cross-shell handoff.
        Choice::OpenTasks => app.main_view = MainView::Tasks,
        Choice::ToggleLayout => {
            app.panel_side = app.panel_side.flipped();
            let _ = app.db.set_panel_side(app.panel_side);
        }
        Choice::SearchProjects => {
            let roots = single_bucket_root(&app.brain_root, Bucket::Projects);
            app.search_rescope(&roots);
        }
        Choice::SearchAreas => {
            let roots = single_bucket_root(&app.brain_root, Bucket::Areas);
            app.search_rescope(&roots);
        }
        Choice::SearchResources => {
            let roots = single_bucket_root(&app.brain_root, Bucket::Resources);
            app.search_rescope(&roots);
        }
        Choice::SearchArchive => {
            let roots = single_bucket_root(&app.brain_root, Bucket::Archive);
            app.search_rescope(&roots);
        }
        Choice::GlobalSearch => {
            let roots = all_bucket_roots(&app.brain_root);
            app.search_rescope(&roots);
        }
        Choice::ToggleReceiver => app.toggle_receiver(),
    }
}

fn create_pdf_inline(app: &App, md: &Path) {
    if let Ok(pdf) = open_target::create_pdf(&app.command_context, md) {
        let _ = open_target::open_with_system(&pdf);
    }
}

/// Open a picked path without tearing down the shell: directories reveal in
/// Finder, text files open in a new iTerm2 tab, everything else hands off to
/// the system `open`. Best-effort — a failed spawn is silently ignored.
fn open_selection(path: &Path) {
    if path.is_dir() {
        let _ = open_target::open_with_system(path);
    } else if open_target::is_textlike(path) {
        let _ = open_target::open_in_editor_tab(path);
    } else {
        let _ = open_target::open_with_system(path);
    }
}

fn reveal_in_finder(path: &Path) {
    let target = open_target::finder_target(path, path.is_file());
    let _ = open_target::open_with_system(target);
}

/// Build the search picker for the brain-directory view over the full bucket
/// set. Called once at startup by `run_tui`.
pub(crate) fn build_search(brain_root: &Path) -> picker::App {
    let entries = entry::collect(brain_root, &all_bucket_roots(brain_root)).unwrap_or_default();
    picker::App::new(&entries, "")
}
