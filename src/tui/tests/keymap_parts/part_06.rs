
#[test]
fn backspace_exits_search_when_query_empty() {
    // The pre-existing behavior Ctrl+U now mirrors.
    assert!(search_edit_key_exits_when_empty(KeyCode::Backspace, false));
}

#[test]
fn bare_u_does_not_exit_empty_search() {
    // Without ctrl, `u` is query text — it never exits search.
    assert!(!search_edit_key_exits_when_empty(KeyCode::Char('u'), false));
}

#[test]
fn bare_letter_does_not_exit_empty_search() {
    assert!(!search_edit_key_exits_when_empty(KeyCode::Char('t'), false));
}

#[test]
fn ctrl_letter_chords_delegate() {
    // Ctrl-modified chords (e.g. Ctrl+D → mark complete, Ctrl+Enter →
    // task actions modal) fall through to normal-mode handling when
    // typed inside `/`.
    assert!(search_delegates_ctrl_chord(KeyCode::Char('r'), true));
    assert!(search_delegates_ctrl_chord(KeyCode::Enter, true));
}
