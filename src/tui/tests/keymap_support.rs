// Tests for the pure key classifiers: count prefix, view shortcuts, search
// delegation, and the app-level chords.

use crate::tasks::view::View;
use crate::tui::keymap::{
    MAX_COUNT, accumulate_count, alt_cycles_brain_tab, alt_scroll_direction,
    alt_selects_brain_tab_slot, ctrl_opens_links, ctrl_opens_palette, ctrl_quits,
    ctrl_removes_task, h_collapses_notes,
    is_count_relevant_key, search_delegates_ctrl_chord, search_edit_key_exits_when_empty,
    search_key_abandons_filter, view_shortcut,
};
use crossterm::event::{KeyCode, KeyModifiers};

// --- accumulate_count (vim-style count prefix) ---
