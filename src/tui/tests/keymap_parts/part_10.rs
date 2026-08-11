
// --- alt_selects_brain_tab_slot ---

#[test]
fn alt_digits_select_brain_tab_slots_in_strip_order() {
    // Slot 0 is the main session; every later digit addresses the nth open
    // skill session, so one binding covers however many tabs are open.
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('1'), KeyModifiers::ALT),
        Some(0)
    );
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('2'), KeyModifiers::ALT),
        Some(1)
    );
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('9'), KeyModifiers::ALT),
        Some(8)
    );
}

#[test]
fn option_generated_macos_digit_glyphs_select_tab_slots() {
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('\u{00a1}'), KeyModifiers::NONE),
        Some(0)
    );
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('\u{2122}'), KeyModifiers::NONE),
        Some(1)
    );
    // Option+3 (£) reaches the third tab on the same layout.
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('\u{00a3}'), KeyModifiers::NONE),
        Some(2)
    );
}

#[test]
fn a_bare_digit_never_selects_a_tab() {
    // The whole reason for Alt over Ctrl: a plain `1`/`2` must stay ordinary
    // input (it types into the brain PTY), never a tab switch.
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('1'), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        alt_selects_brain_tab_slot(KeyCode::Char('0'), KeyModifiers::ALT),
        None
    );
}

// --- alt_cycles_brain_tab ---

#[test]
fn alt_bracket_keys_cycle_the_brain_tab() {
    assert_eq!(
        alt_cycles_brain_tab(KeyCode::Char(']'), KeyModifiers::ALT),
        Some(true)
    );
    assert_eq!(
        alt_cycles_brain_tab(KeyCode::Char('['), KeyModifiers::ALT),
        Some(false)
    );
}
