//! The brain-directory (fuzzy search) main view's key handling and actions.
//!
//! Ported from the pre-merge standalone brain shell. Drives the embedded
//! `picker::App` (owned by `ShellState`) stores its query, navigation, and in-place file
//! opening. Search palette and confirmation data live in the shell's single
//! overlay slot. Only invoked while `main_view == MainView::BrainSearch`
//! and the main panel is focused; the app-level chords (view switching,
//! brain-panel open/close/new, `Alt+S` help, `Ctrl+Q` quit) are intercepted
//! upstream in `event_loop` and never reach here.

use std::path::Path;

use crossterm::event::KeyEvent;

use super::{App, Overlay, SearchEffect, ShellState, close_overlay, open_overlay, replace_overlay};
use crate::entry::{self, Bucket};
use crate::menu::SearchAction;
use crate::open_target;
use crate::tui::PaletteStep;
use crate::{confirm, picker};

impl App {
    /// Re-walk `roots` into the search picker, clearing the query (a scope
    /// switch from the brain-search palette).
    pub(crate) fn search_rescope(&mut self, roots: &[(Bucket, std::path::PathBuf)]) {
        if let Ok(entries) = entry::collect(self.context.workspace_root(), roots) {
            self.shell.replace_search_entries(&entries);
        }
    }

    /// Re-walk the full bucket set into the search picker, keeping the query
    /// (`Ctrl-R`, or after a PDF is created / an entry is trashed).
    pub(crate) fn search_refresh(&mut self) {
        if let Ok(entries) = entry::collect(
            self.context.workspace_root(),
            &all_bucket_roots(self.context.workspace_root()),
        ) {
            self.shell.reload_search_entries(&entries);
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

pub(crate) fn handle_search_view_key(
    shell: &mut ShellState,
    k: &KeyEvent,
    ctrl: bool,
    alt: bool,
) -> SearchEffect {
    shell.handle_search_input(k.code, ctrl, alt)
}

pub(crate) fn apply_search_view_effect(app: &mut App, effect: SearchEffect) -> bool {
    match effect {
        SearchEffect::None => {}
        SearchEffect::Quit => return true,
        SearchEffect::Open(path) => open_selection(&path),
        SearchEffect::Reveal(path) => reveal_in_finder(&path),
        SearchEffect::OpenPalette => {
            app.refresh_receiver_enabled();
            let palette = app
                .shell
                .search_palette(!app.brain_panel_open(), app.receiver.is_enabled());
            open_overlay(&mut app.overlay, Overlay::SearchPalette(palette));
        }
        SearchEffect::ConfirmPdf(path) => {
            let confirm = picker::App::pdf_confirmation(path);
            open_overlay(&mut app.overlay, Overlay::SearchConfirmation(confirm));
        }
        SearchEffect::Refresh => app.search_refresh(),
        SearchEffect::ConfirmDelete(path) => {
            let confirm = picker::App::delete_confirmation(path);
            open_overlay(&mut app.overlay, Overlay::SearchConfirmation(confirm));
        }
    }
    false
}

pub(crate) fn route_search_palette(app: &mut App, k: &KeyEvent) {
    let Some(Overlay::SearchPalette(palette)) = app.overlay.as_mut() else {
        return;
    };
    match palette.handle_key(*k) {
        PaletteStep::Continue => {}
        PaletteStep::Cancel => {
            close_overlay(&mut app.overlay);
        }
        PaletteStep::Confirm(action) => {
            if action == SearchAction::Delete {
                if let Some(path) = app.shell.selected_search_path() {
                    let confirm = picker::App::delete_confirmation(path);
                    replace_overlay(&mut app.overlay, Overlay::SearchConfirmation(confirm));
                } else {
                    close_overlay(&mut app.overlay);
                }
                return;
            }
            close_overlay(&mut app.overlay);
            app.execute_search_action(action);
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

impl App {
    fn execute_search_action(&mut self, action: SearchAction) {
        match action {
            SearchAction::Global(action) => self.execute_global_action(action),
            SearchAction::CreatePdf => {
                if let Some(path) = self.shell.selected_markdown_search_path() {
                    create_pdf_inline(self, &path);
                    self.search_refresh();
                }
            }
            SearchAction::OpenFile => {
                if let Some(path) = self.shell.selected_search_path() {
                    open_selection(&path);
                }
            }
            SearchAction::OpenDir => {
                if let Some(path) = self.shell.selected_search_path() {
                    reveal_in_finder(&path);
                }
            }
            SearchAction::Delete => {}
            SearchAction::SearchProjects => {
                let roots = single_bucket_root(self.context.workspace_root(), Bucket::Projects);
                self.search_rescope(&roots);
            }
            SearchAction::SearchAreas => {
                let roots = single_bucket_root(self.context.workspace_root(), Bucket::Areas);
                self.search_rescope(&roots);
            }
            SearchAction::SearchResources => {
                let roots = single_bucket_root(self.context.workspace_root(), Bucket::Resources);
                self.search_rescope(&roots);
            }
            SearchAction::SearchArchive => {
                let roots = single_bucket_root(self.context.workspace_root(), Bucket::Archive);
                self.search_rescope(&roots);
            }
            SearchAction::GlobalSearch => {
                let roots = all_bucket_roots(self.context.workspace_root());
                self.search_rescope(&roots);
            }
        }
    }
}

fn create_pdf_inline(app: &App, md: &Path) {
    if let Ok(pdf) = open_target::create_pdf(app.context.command(), md) {
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
