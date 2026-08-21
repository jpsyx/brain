#[test]
fn open_links_hidden_when_task_has_no_links() {
    let state = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::None,
    );
    assert!(!has_open_links(&state));
}

#[test]
fn open_links_single_linear_label() {
    // Actions modal (no id in the label) and global palette (named).
    let actions = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::SingleLinear,
    );
    assert!(has_open_links(&actions));
    assert_eq!(
        open_links_label(&actions).as_deref(),
        Some("Open Linear link")
    );

    let global = TaskPalette::new(
        Some("T123".into()),
        false,
        false,
        false,
        LinkKind::SingleLinear,
        false,
        false,
    );
    assert_eq!(
        open_links_label(&global).as_deref(),
        Some("Open T123 Linear link")
    );
}

#[test]
fn open_links_single_notes_label() {
    let actions = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::SingleNotes,
    );
    assert!(has_open_links(&actions));
    assert_eq!(
        open_links_label(&actions).as_deref(),
        Some("Open link from note")
    );

    let global = TaskPalette::new(
        Some("T90".into()),
        false,
        false,
        false,
        LinkKind::SingleNotes,
        false,
        false,
    );
    assert_eq!(
        open_links_label(&global).as_deref(),
        Some("Open link from T90's note")
    );
}

#[test]
fn open_links_multiple_label() {
    let actions = TaskPalette::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::Multiple,
    );
    assert!(has_open_links(&actions));
    assert_eq!(
        open_links_label(&actions).as_deref(),
        Some("Open attached link")
    );

    let global = TaskPalette::new(
        Some("T123".into()),
        false,
        false,
        false,
        LinkKind::Multiple,
        false,
        false,
    );
    assert_eq!(
        open_links_label(&global).as_deref(),
        Some("Open link attached to T123")
    );
}
