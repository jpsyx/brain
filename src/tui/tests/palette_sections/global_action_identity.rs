#[test]
fn application_rows_use_the_global_action_identity() {
    let state = TaskPalette::new(None, false, false, false, LinkKind::None, false, false);
    let actions = action_order(&state);

    assert!(actions.contains(&TaskAction::Global(GlobalAction::OpenHabits)));
    assert!(actions.contains(&TaskAction::Global(GlobalAction::OpenAgenda)));
    assert_eq!(
        shortcut_for(TaskAction::Global(GlobalAction::OpenHabits)),
        Some("^H")
    );
    assert_eq!(
        shortcut_for(TaskAction::Global(GlobalAction::OpenAgenda)),
        Some("^A")
    );
}
