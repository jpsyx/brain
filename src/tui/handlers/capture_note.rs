//! The capture-note modal's captive key handler.
//!
//! Everything but the final spawn lives on [`App::submit_capture_note`], which
//! writes the note, refreshes the brain directory, and hands back the path;
//! opening it is the same fire-and-forget editor tab the brain-directory view
//! uses for any other text file.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::App;
use crate::tui::overlay::{Overlay, close_overlay};
use crate::tui::search_view::open_selection;

pub(crate) fn handle_capture_note_key(app: &mut App, key: &crossterm::event::KeyEvent) {
    let Some(Overlay::CaptureNote(state)) = app.overlay.as_mut() else {
        return;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('c' | 'C') if ctrl => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Enter => {
            if let Some(path) = app.submit_capture_note() {
                open_selection(&path);
            }
        }
        KeyCode::Char('u' | 'U') if ctrl => state.clear(),
        KeyCode::Backspace => state.pop(),
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
                && !character.is_control() =>
        {
            state.push(character);
        }
        _ => {}
    }
}
