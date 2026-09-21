fn task_actions(context: &PaletteContext) -> CommandPaletteState {
    CommandPaletteState::new_task_actions(context)
}

#[test]
fn the_actions_modal_lists_only_task_commands_in_catalog_order() {
    let state = task_actions(&with_task(task("T1", true, false, LinkKind::None)));

    assert_eq!(
        actions(&state),
        vec![
            Command::Task(TaskCommand::Start),
            Command::Task(TaskCommand::MarkComplete),
            Command::Task(TaskCommand::MessageBrainAbout),
            Command::Task(TaskCommand::ToggleNotes),
            Command::Task(TaskCommand::OpenLinks),
            Command::Task(TaskCommand::Remove),
            Command::Task(TaskCommand::Reassign),
            Command::Task(TaskCommand::Defer(1)),
            Command::Task(TaskCommand::Defer(7)),
            Command::Task(TaskCommand::Defer(14)),
        ]
    );
}

#[test]
fn the_actions_modal_drops_the_id_because_its_title_carries_it() {
    let state = task_actions(&with_task(task("T1", true, true, LinkKind::Multiple)));

    assert_eq!(state.title(), "Task T1 actions");
    assert_eq!(state.subtitle(), Some("a task"));
    assert_eq!(
        label_of(&state, Command::Task(TaskCommand::MarkComplete)).as_deref(),
        Some("Mark as complete")
    );
    assert_eq!(
        label_of(&state, Command::Task(TaskCommand::ToggleNotes)).as_deref(),
        Some("Collapse notes")
    );
    assert_eq!(
        label_of(&state, Command::Task(TaskCommand::OpenLinks)).as_deref(),
        Some("Open attached link")
    );
}

#[test]
fn the_actions_modal_hides_tasks_only_commands_for_a_habit() {
    // Unlike the global palette, this modal is already committed to one row,
    // so a command that cannot apply to it is not shown at all.
    let state = task_actions(&with_task(task("H1", false, false, LinkKind::SingleNotes)));
    let listed = actions(&state);

    assert!(!listed.contains(&Command::Task(TaskCommand::Defer(1))));
    assert!(!listed.contains(&Command::Task(TaskCommand::Remove)));
    assert!(!listed.contains(&Command::Task(TaskCommand::Start)));
    assert!(listed.contains(&Command::Task(TaskCommand::MarkComplete)));
    assert!(listed.contains(&Command::Task(TaskCommand::OpenLinks)));
}

#[test]
fn the_global_palette_keeps_its_own_title_and_no_subtitle() {
    let state = palette(&with_task(task("T1", false, false, LinkKind::None)));
    assert_eq!(state.title(), "Command palette");
    assert!(!state.task_actions_modal());
}
