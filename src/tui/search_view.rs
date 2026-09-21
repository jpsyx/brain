//! The brain-directory (fuzzy search) main view's key handling and actions.
//!
//! Ported from the pre-merge standalone brain shell. Drives the embedded
//! `picker::App` (owned by `ShellState`) and stores its query, navigation, and
//! in-place file opening. Its direct keys resolve a path and hand it to the
//! same [`EntryCommand`](crate::tui::palette::EntryCommand) the palette runs,
//! so a shortcut and its palette row can never drift apart. Only invoked while
//! `main_view == MainView::BrainSearch` and the main panel is focused; the
//! app-level chords (view switching, brain-panel open/close/new, `Alt+S` help,
//! `Ctrl+Q` quit) are intercepted upstream in `event_loop`.

use std::path::Path;

use crossterm::event::KeyEvent;

use crate::entry::{self, Bucket};
use crate::open_target;
use crate::tui::App;
use crate::tui::overlay::{Overlay, close_overlay, open_overlay};
use crate::tui::palette::{CommandPaletteState, EntryCommand};
use crate::tui::state::{SearchEffect, ShellState};
use crate::{confirm, picker};

impl App {
    /// Re-walk `roots` into the search picker, clearing the query (a scope
    /// switch from the command palette).
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
        (Bucket::Capture, brain_root.join("capture")),
        (Bucket::Projects, brain_root.join("projects")),
        (Bucket::Areas, brain_root.join("areas")),
        (Bucket::Resources, brain_root.join("resources")),
        (Bucket::Archive, brain_root.join("archive")),
    ]
}

pub(crate) fn single_bucket_root(
    brain_root: &Path,
    bucket: Bucket,
) -> Vec<(Bucket, std::path::PathBuf)> {
    let dir = match bucket {
        Bucket::Capture => "capture",
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
        SearchEffect::Open(path) => app.run_entry_command(EntryCommand::Open, &path),
        SearchEffect::Reveal(path) => app.run_entry_command(EntryCommand::Reveal, &path),
        SearchEffect::OpenPalette => {
            let context = app.palette_context();
            open_overlay(
                &mut app.overlay,
                Overlay::CommandPalette(CommandPaletteState::new(&context)),
            );
        }
        SearchEffect::ConfirmPdf(path) => app.run_entry_command(EntryCommand::CreatePdf, &path),
        SearchEffect::Refresh => app.search_refresh(),
        SearchEffect::ConfirmDelete(path) => app.run_entry_command(EntryCommand::Delete, &path),
    }
    false
}

pub(crate) fn route_search_confirm(app: &mut App, k: &KeyEvent) {
    let Some(Overlay::SearchConfirmation(confirm)) = app.overlay.as_mut() else {
        return;
    };
    match confirm::handle_key(confirm, *k) {
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

pub(crate) fn create_pdf_inline(app: &App, md: &Path) {
    if let Ok(pdf) = open_target::create_pdf(app.context.command(), md) {
        let _ = open_target::open_with_system(&pdf);
    }
}

/// Open a picked path without tearing down the shell: directories reveal in
/// Finder, text files open in a new iTerm2 tab, everything else hands off to
/// the system `open`. Best-effort — a failed spawn is silently ignored.
pub(crate) fn open_selection(path: &Path) {
    if path.is_dir() {
        let _ = open_target::open_with_system(path);
    } else if open_target::is_textlike(path) {
        let _ = open_target::open_in_editor_tab(path);
    } else {
        let _ = open_target::open_with_system(path);
    }
}

pub(crate) fn reveal_in_finder(path: &Path) {
    let target = open_target::finder_target(path, path.is_file());
    let _ = open_target::open_with_system(target);
}

/// Build the search picker for the brain-directory view over the full bucket
/// set. Called once at startup by `run_tui`.
pub(crate) fn build_search(brain_root: &Path) -> picker::App {
    let entries = entry::collect(brain_root, &all_bucket_roots(brain_root)).unwrap_or_default();
    picker::App::new(&entries, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_search_covers_the_capture_in_basket_and_every_para_bucket() {
        // `capture/` is searchable like any other bucket: the user dumps notes
        // and files there and needs to find them again before they are filed.
        let roots = all_bucket_roots(Path::new("/brain"));

        assert_eq!(
            roots,
            vec![
                (Bucket::Capture, std::path::PathBuf::from("/brain/capture")),
                (
                    Bucket::Projects,
                    std::path::PathBuf::from("/brain/projects")
                ),
                (Bucket::Areas, std::path::PathBuf::from("/brain/areas")),
                (
                    Bucket::Resources,
                    std::path::PathBuf::from("/brain/resources")
                ),
                (Bucket::Archive, std::path::PathBuf::from("/brain/archive")),
            ]
        );
    }

    #[test]
    fn rescoping_to_capture_walks_only_the_capture_directory() {
        assert_eq!(
            single_bucket_root(Path::new("/brain"), Bucket::Capture),
            vec![(Bucket::Capture, std::path::PathBuf::from("/brain/capture"))]
        );
    }
}
