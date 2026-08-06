
#[test]
fn bare_c_in_search_is_text_input() {
    // A bare `c` is query text, not an abandon-search chord.
    assert!(!search_key_abandons_filter(KeyCode::Char('c'), false));
}

#[test]
fn ctrl_u_in_search_does_not_abandon_filter() {
    // Ctrl+U clears the query but stays in search mode — it is not an
    // abandon-and-exit chord.
    assert!(!search_key_abandons_filter(KeyCode::Char('u'), true));
}

#[test]
fn ctrl_u_keeps_search_specific_handling() {
    // Ctrl+U clears the query in search mode (readline-style). It
    // must NOT fall through to normal-mode's bare-`u` half-page-up
    // navigation.
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('u'), true));
}

// --- search_edit_key_exits_when_empty ---

#[test]
fn ctrl_u_exits_search_when_query_empty() {
    // On an empty query, Ctrl+U has nothing to clear, so it doubles as
    // an exit — the same "press again to leave" behavior as Backspace.
    assert!(search_edit_key_exits_when_empty(KeyCode::Char('u'), true));
}
