//! Running an [`EntryCommand`] against one resolved brain-directory path.

use std::path::Path;

use crate::open_target;
use crate::tui::App;
use crate::tui::modal_state::FlashKind;
use crate::tui::overlay::{Overlay, open_overlay};
use crate::tui::palette::{EntryCommand, EntryContext};
use crate::tui::search_view::{open_selection, reveal_in_finder};

impl App {
    pub(crate) fn run_entry_command(&mut self, command: EntryCommand, path: &Path) {
        if !entry_context_for(path).satisfies(command.requirement()) {
            self.status.set_flash(FlashKind::Error(format!(
                "⚠ {}",
                command.requirement_error()
            )));
            return;
        }
        match command {
            EntryCommand::Open => open_selection(path),
            EntryCommand::Reveal => reveal_in_finder(path),
            // Inert until the tree sub-view exists to switch to. The command
            // is declared first so the palette, its wording, and its target
            // picker can be built and tested against it.
            EntryCommand::Explore => {}
            EntryCommand::CopyFilePath => self.copy_path(path),
            EntryCommand::CopyDirPath => {
                let target = open_target::finder_target(path, path.is_file());
                self.copy_path(target);
            }
            EntryCommand::CreatePdf => {
                open_overlay(
                    &mut self.overlay,
                    Overlay::SearchConfirmation(crate::confirm::Confirm::pdf(path.to_path_buf())),
                );
            }
            EntryCommand::Delete => {
                open_overlay(
                    &mut self.overlay,
                    Overlay::SearchConfirmation(crate::confirm::Confirm::delete(
                        path.to_path_buf(),
                    )),
                );
            }
        }
    }

    fn copy_path(&mut self, path: &Path) {
        let flash = match open_target::copy_to_clipboard(path) {
            Ok(()) => FlashKind::Info(format!("✓ copied {}", path.display())),
            Err(error) => FlashKind::Error(format!("⚠ copy failed: {error}")),
        };
        self.status.set_flash(flash);
    }
}

/// The requirement-checking view of a path on disk. Only the three fields the
/// gate reads are filled in; labels come from the palette's own context.
fn entry_context_for(path: &Path) -> EntryContext {
    let is_file = path.is_file();
    EntryContext {
        filename: String::new(),
        dir_reldisplay: String::new(),
        is_file,
        is_markdown: is_file && open_target::is_markdown(path),
    }
}
