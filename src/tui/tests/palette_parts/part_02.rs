
#[test]
fn notes_toggle_in_global_palette_names_the_task() {
    // In the global command palette the toggle follows the task-ID convention
    // of the other task-specific commands ("Expand T123 notes").
    let state = PaletteState::new(
        Some("T123".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    assert_eq!(toggle_label(&state).as_deref(), Some("Expand T123 notes"));
}

#[test]
fn notes_toggle_in_global_palette_reads_collapse_when_expanded() {
    let state = PaletteState::new(
        Some("T123".into()),
        false,
        true,
        true,
        LinkKind::None,
        false,
        false,
    );
    assert_eq!(toggle_label(&state).as_deref(), Some("Collapse T123 notes"));
}

// --- PaletteState: "open link" gating + per-kind label ---

fn has_open_links(state: &PaletteState) -> bool {
    state
        .visible()
        .iter()
        .any(|c| matches!(c.action, PaletteAction::OpenLinks))
}

fn open_links_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::OpenLinks))
        .map(|row| row.label.clone())
}
