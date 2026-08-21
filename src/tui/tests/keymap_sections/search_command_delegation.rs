
#[test]
fn h_switches_to_habits_when_notes_collapsed() {
    // Entry has notes but they're collapsed — `h` is the habits
    // shortcut as usual.
    assert!(!h_collapses_notes(true, false));
}

#[test]
fn h_switches_to_habits_when_entry_has_no_notes() {
    // No notes to collapse (even if `full_notes` reports "expanded"),
    // so `h` stays the habits shortcut and never dead-ends.
    assert!(!h_collapses_notes(false, true));
    assert!(!h_collapses_notes(false, false));
}

// --- search_delegates_ctrl_chord ---

#[test]
fn ctrl_enter_delegates_to_normal_in_search_mode() {
    // The motivating case: Ctrl+Enter opens the task actions modal on
    // the highlighted task even while the user is typing in the search
    // input (bare Enter exits search, so it can't do double duty).
    assert!(search_delegates_ctrl_chord(KeyCode::Enter, true));
}

#[test]
fn ctrl_d_delegates_to_normal_in_search_mode() {
    // Ctrl+D ("done") must mark-complete the highlighted task from
    // inside `/` without first exiting search.
    assert!(search_delegates_ctrl_chord(KeyCode::Char('d'), true));
}
