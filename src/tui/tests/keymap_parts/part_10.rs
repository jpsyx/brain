// --- alt_selects_brain_tab_slot ---

#[test]
fn alt_digits_select_brain_tab_slots_in_strip_order() {
    // Slot 0 is the main session; every later digit addresses the nth open
    // skill session, so one binding covers however many tabs are open.
    let slot = |code, modifiers| alt_selects_brain_tab_slot(code, modifiers).map(|s| s.index);
    assert_eq!(slot(KeyCode::Char('1'), KeyModifiers::ALT), Some(0));
    assert_eq!(slot(KeyCode::Char('2'), KeyModifiers::ALT), Some(1));
    assert_eq!(slot(KeyCode::Char('9'), KeyModifiers::ALT), Some(8));
    // An Alt chord is a deliberate tab request, so the shell may consume it even
    // when that slot holds no tab.
    assert!(
        alt_selects_brain_tab_slot(KeyCode::Char('9'), KeyModifiers::ALT)
            .expect("slot")
            .from_chord
    );
}

#[test]
fn option_generated_macos_digit_glyphs_select_tab_slots() {
    let slot = |code| alt_selects_brain_tab_slot(code, KeyModifiers::NONE).map(|s| s.index);
    assert_eq!(slot(KeyCode::Char('\u{00a1}')), Some(0));
    assert_eq!(slot(KeyCode::Char('\u{2122}')), Some(1));
    // Option+3 (£) reaches the third tab on the same layout.
    assert_eq!(slot(KeyCode::Char('\u{00a3}')), Some(2));
}

#[test]
fn an_option_glyph_is_never_treated_as_a_deliberate_chord() {
    // `£`, `•`, `§` and friends are ordinary typeable characters. Flagging them
    // as not-a-chord is what lets the shell forward them to the panel when no
    // such tab is open, instead of swallowing the keystroke.
    for glyph in ['\u{00a1}', '\u{2122}', '\u{00a3}', '\u{2022}', '\u{00a7}'] {
        let slot = alt_selects_brain_tab_slot(KeyCode::Char(glyph), KeyModifiers::NONE)
            .unwrap_or_else(|| panic!("{glyph} should address a tab slot"));
        assert!(!slot.from_chord, "{glyph}");
    }
}

#[test]
fn a_bare_digit_never_selects_a_tab() {
    // The whole reason for Alt over Ctrl: a plain `1`/`2` must stay ordinary
    // input (it types into the brain PTY), never a tab switch.
    assert!(alt_selects_brain_tab_slot(KeyCode::Char('1'), KeyModifiers::NONE).is_none());
    assert!(alt_selects_brain_tab_slot(KeyCode::Char('0'), KeyModifiers::ALT).is_none());
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
