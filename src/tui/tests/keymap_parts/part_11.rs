
#[test]
fn option_generated_macos_quote_glyphs_cycle_the_brain_tab() {
    // On US layouts Option+] / Option+[ surface as smart-quote glyphs with no
    // modifier; those are the reliable path on such terminals.
    assert_eq!(
        alt_cycles_brain_tab(KeyCode::Char('\u{2018}'), KeyModifiers::NONE),
        Some(true)
    );
    assert_eq!(
        alt_cycles_brain_tab(KeyCode::Char('\u{201C}'), KeyModifiers::NONE),
        Some(false)
    );
}

#[test]
fn a_bare_bracket_never_cycles_the_brain_tab() {
    assert_eq!(
        alt_cycles_brain_tab(KeyCode::Char('['), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        alt_cycles_brain_tab(KeyCode::Char('a'), KeyModifiers::ALT),
        None
    );
}

// --- ctrl_opens_links ---

#[test]
fn ctrl_o_opens_links() {
    assert!(ctrl_opens_links(KeyCode::Char('o'), true));
    assert!(ctrl_opens_links(KeyCode::Char('O'), true));
}

#[test]
fn bare_o_does_not_open_links() {
    // Without ctrl, `o` is an ordinary key — never the open action.
    assert!(!ctrl_opens_links(KeyCode::Char('o'), false));
    assert!(!ctrl_opens_links(KeyCode::Char('O'), false));
}
