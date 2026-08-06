
// --- alt_selects_brain_tab ---

#[test]
fn alt_1_and_alt_2_select_brain_tabs() {
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('1'), KeyModifiers::ALT),
        Some(BrainTab::Main)
    );
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('2'), KeyModifiers::ALT),
        Some(BrainTab::Triage)
    );
}

#[test]
fn option_generated_macos_digit_glyphs_select_tabs() {
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('\u{00a1}'), KeyModifiers::NONE),
        Some(BrainTab::Main)
    );
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('\u{2122}'), KeyModifiers::NONE),
        Some(BrainTab::Triage)
    );
}

#[test]
fn a_bare_digit_never_selects_a_tab() {
    // The whole reason for Alt over Ctrl: a plain `1`/`2` must stay ordinary
    // input (it types into the brain PTY), never a tab switch.
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('1'), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('3'), KeyModifiers::ALT),
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
