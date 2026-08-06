
#[test]
fn bare_letter_stays_in_search_mode() {
    // Without ctrl, letters are text input for the query — never
    // a normal-mode shortcut.
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('t'), false));
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('a'), false));
}

// --- view_shortcut ---

#[test]
fn view_shortcut_bare_letters_map_to_views() {
    assert_eq!(view_shortcut(KeyCode::Char('t'), false), Some(View::Today));
    assert_eq!(view_shortcut(KeyCode::Char('m'), false), Some(View::Mit));
    assert_eq!(
        view_shortcut(KeyCode::Char('p'), false),
        Some(View::PastDue)
    );
    assert_eq!(view_shortcut(KeyCode::Char('w'), false), Some(View::Week));
    assert_eq!(view_shortcut(KeyCode::Char('a'), false), Some(View::All));
}

#[test]
fn view_shortcut_ctrl_modified_never_switches_views() {
    // Ctrl+<letter> must not switch views — otherwise Ctrl+P would
    // collide with the command-palette chord.
    for c in ['t', 'm', 'p', 'w', 'a'] {
        assert_eq!(view_shortcut(KeyCode::Char(c), true), None);
    }
}

#[test]
fn view_shortcut_ignores_unrelated_keys() {
    assert_eq!(view_shortcut(KeyCode::Char('z'), false), None);
    assert_eq!(view_shortcut(KeyCode::Enter, false), None);
}
