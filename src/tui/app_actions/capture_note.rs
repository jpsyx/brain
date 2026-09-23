//! Creating a note in the `capture/` in-basket from the command palette.

use std::path::PathBuf;

use crate::tui::App;
use crate::tui::modal_state::{CaptureNoteState, FlashKind};
use crate::tui::overlay::{Overlay, close_overlay, open_overlay};

impl App {
    /// Raise the modal that asks what to call the note.
    pub(crate) fn open_capture_note_input(&mut self) {
        open_overlay(
            &mut self.overlay,
            Overlay::CaptureNote(CaptureNoteState::new(crate::capture_note::now_timestamp())),
        );
    }

    /// Write the note the modal describes and return its path for opening.
    ///
    /// Closes the modal and refreshes the brain directory on success, so the
    /// new note is findable the moment the editor tab comes up. A failed write
    /// leaves the modal up carrying the reason, since the text the user typed
    /// is the only copy of it.
    pub(crate) fn submit_capture_note(&mut self) -> Option<PathBuf> {
        let Some(Overlay::CaptureNote(state)) = self.overlay.as_ref() else {
            return None;
        };
        let root = self.context.workspace_root().to_path_buf();
        let created = crate::capture_note::create(&root, state.buffer(), state.timestamp());
        match created {
            Ok(path) => {
                close_overlay(&mut self.overlay);
                self.search_refresh();
                let shown = path.strip_prefix(&root).unwrap_or(&path).display();
                self.status
                    .set_flash(FlashKind::Info(format!("✓ created {shown}")));
                Some(path)
            }
            Err(error) => {
                if let Some(Overlay::CaptureNote(state)) = self.overlay.as_mut() {
                    state.error = Some(format!("Note could not be created: {error:#}"));
                }
                None
            }
        }
    }
}
