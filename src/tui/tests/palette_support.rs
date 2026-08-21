// Tests for TaskPalette: notes toggle, open-link gating/labels, action
// order, and numbered rows.

use crate::tui::*;

// --- TaskPalette: notes toggle ---

fn has_toggle(state: &TaskPalette) -> bool {
    state
        .visible()
        .iter()
        .any(|c| matches!(c.action, TaskAction::ToggleNotes))
}

fn toggle_label(state: &TaskPalette) -> Option<String> {
    state
        .visible()
        .into_iter()
        .find(|row| matches!(row.action, TaskAction::ToggleNotes))
        .map(|row| row.label.clone())
}
