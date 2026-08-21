
#[test]
fn count_is_capped_at_max() {
    // Runaway digit entry saturates rather than overflowing.
    let mut c = Some(MAX_COUNT);
    c = accumulate_count(c, 9);
    assert_eq!(c, Some(MAX_COUNT));
}

#[test]
fn digits_and_motions_preserve_a_pending_count() {
    for c in ['0', '5', '9', 'j', 'k'] {
        assert!(is_count_relevant_key(KeyCode::Char(c), false));
    }
    assert!(is_count_relevant_key(KeyCode::Up, false));
    assert!(is_count_relevant_key(KeyCode::Down, false));
}

#[test]
fn other_keys_clear_a_pending_count() {
    // Non-motion normal keys, and any ctrl-modified key, are not
    // count-relevant — they clear the prefix.
    assert!(!is_count_relevant_key(KeyCode::Char('g'), false));
    assert!(!is_count_relevant_key(KeyCode::Char('d'), false));
    assert!(!is_count_relevant_key(KeyCode::Char('l'), false));
    assert!(!is_count_relevant_key(KeyCode::Enter, false));
    assert!(!is_count_relevant_key(KeyCode::Char('/'), false));
    // Ctrl chords never participate, even on otherwise-relevant keys.
    assert!(!is_count_relevant_key(KeyCode::Char('j'), true));
    assert!(!is_count_relevant_key(KeyCode::Char('5'), true));
}

// --- h_collapses_notes ---

#[test]
fn h_collapses_when_highlighted_entry_has_expanded_notes() {
    // The motivating case: notes are expanded on the highlighted
    // entry, so `h` must collapse them rather than jump to habits.
    assert!(h_collapses_notes(true, true));
}
