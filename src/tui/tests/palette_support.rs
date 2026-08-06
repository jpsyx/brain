// Tests for PaletteState: notes toggle, open-link gating/labels, action
// order, and numbered rows.

use crate::tui::*;

// --- PaletteState: notes toggle ---

fn has_toggle(state: &PaletteState) -> bool {
    state
        .visible()
        .iter()
        .any(|c| matches!(c.action, PaletteAction::ToggleNotes))
}

fn toggle_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::ToggleNotes))
        .map(|c| state.label_for(c))
}
