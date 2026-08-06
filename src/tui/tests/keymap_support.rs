// Tests for the pure key classifiers: count prefix, view shortcuts, search
// delegation, and the app-level chords.

use crate::tasks::view::View;
use crate::tui::*;
use crossterm::event::{KeyCode, KeyModifiers};

// --- accumulate_count (vim-style count prefix) ---
