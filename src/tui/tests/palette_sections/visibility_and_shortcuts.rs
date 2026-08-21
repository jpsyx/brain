#[test]
fn open_links_advertises_its_ctrl_o_shortcut() {
    // The `[^O]` hint renders next to the label in both modals, mirroring
    // the other directly-bound actions (^D, ^N, …).
    assert_eq!(shortcut_for(TaskAction::OpenLinks), Some("^O"));
}

#[test]
fn assignment_palette_controls_are_visible_only_for_shared_workspaces() {
    let personal = TaskPalette::new(
        Some("T1".into()),
        false,
        false,
        false,
        LinkKind::None,
        false,
        false,
    );
    let shared = TaskPalette::new(
        Some("T1".into()),
        false,
        false,
        false,
        LinkKind::None,
        false,
        false,
    )
    .with_assignment_mode(crate::tasks::task::AssignmentUiMode {
        show_in_detail: true,
        show_create_control: true,
        show_reassign_control: true,
        show_filter: true,
    });

    for action in [
        TaskAction::AddTask,
        TaskAction::ReassignTask,
        TaskAction::ChooseAssigneeFilter,
    ] {
        assert!(!action_order(&personal).contains(&action));
        assert!(action_order(&shared).contains(&action));
        assert_eq!(shortcut_for(action), None);
    }
}

#[test]
fn assignment_palette_uses_each_surface_visibility_flag_independently() {
    let asymmetric = TaskPalette::new(
        Some("T1".into()),
        false,
        false,
        false,
        LinkKind::None,
        false,
        false,
    )
    .with_assignment_mode(crate::tasks::task::AssignmentUiMode {
        show_in_detail: false,
        show_create_control: true,
        show_reassign_control: false,
        show_filter: true,
    });
    let actions = action_order(&asymmetric);

    assert!(actions.contains(&TaskAction::AddTask));
    assert!(!actions.contains(&TaskAction::ReassignTask));
    assert!(actions.contains(&TaskAction::ChooseAssigneeFilter));
}

#[test]
fn brain_logs_are_always_available() {
    let without_logs = TaskPalette::new(None, false, false, false, LinkKind::None, false, false);
    assert!(
        action_order(&without_logs).contains(&TaskAction::Global(GlobalAction::ShowBrainLogs)),
        "Brain logs should always be available as a diagnostic view"
    );
}
