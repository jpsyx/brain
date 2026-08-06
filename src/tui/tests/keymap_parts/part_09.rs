
#[test]
fn ctrl_other_keys_do_not_quit() {
    assert!(!ctrl_quits(KeyCode::Char('c'), true));
    assert!(!ctrl_quits(KeyCode::Char('x'), true));
}

// --- alt_scroll_direction ---

#[test]
fn alt_u_and_alt_d_scroll_by_focused_panel() {
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('u'), KeyModifiers::ALT),
        Some(true)
    );
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('D'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        Some(false)
    );
}

#[test]
fn option_generated_macos_glyphs_scroll_too() {
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('\u{00a8}'), KeyModifiers::NONE),
        Some(true)
    );
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('\u{2202}'), KeyModifiers::NONE),
        Some(false)
    );
}

#[test]
fn non_scroll_alt_keys_are_ignored() {
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('s'), KeyModifiers::ALT),
        None
    );
    assert_eq!(
        alt_scroll_direction(KeyCode::Enter, KeyModifiers::ALT),
        None
    );
}
