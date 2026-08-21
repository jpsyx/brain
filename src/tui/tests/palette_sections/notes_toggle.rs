#[test]
fn notes_toggle_hidden_when_task_has_no_notes() {
    let state = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::None,
    );
    assert!(!has_toggle(&state));
}

#[test]
fn notes_toggle_shown_and_reads_expand_when_collapsed() {
    let state = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        true,
        false,
        LinkKind::None,
    );
    assert!(has_toggle(&state));
    assert_eq!(toggle_label(&state).as_deref(), Some("Expand notes"));
}

#[test]
fn notes_toggle_reads_collapse_when_expanded() {
    let state = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        true,
        true,
        LinkKind::None,
    );
    assert_eq!(toggle_label(&state).as_deref(), Some("Collapse notes"));
}

#[test]
fn notes_toggle_available_for_habits_with_notes() {
    // Habits can carry notes too; the toggle is `works_on_habits`.
    let state = TaskPalette::new_task_actions(
        "H1".into(),
        "habit".into(),
        true,
        true,
        false,
        LinkKind::None,
    );
    assert!(has_toggle(&state));
}
