#[test]
fn a_highlighted_task_makes_every_task_row_name_it() {
    let state = palette(&with_task(task("T123", true, false, LinkKind::SingleLinear)));

    let cases = [
        (TaskCommand::Start, "Start T123"),
        (TaskCommand::MarkComplete, "Mark T123 as complete"),
        (
            TaskCommand::MessageBrainAbout,
            "Message brain about T123",
        ),
        (TaskCommand::ToggleNotes, "Expand T123 notes"),
        (TaskCommand::OpenLinks, "Open T123 Linear link"),
        (TaskCommand::Remove, "Remove task T123"),
        (TaskCommand::Reassign, "Reassign T123"),
        (TaskCommand::Defer(7), "Defer T123 +7d"),
    ];
    for (command, expected) in cases {
        assert_eq!(
            label_of(&state, Command::Task(command)).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn no_highlighted_task_makes_every_task_row_read_generically() {
    let state = palette(&shared_workspace());

    let cases = [
        (TaskCommand::Start, "Start a task"),
        (TaskCommand::MarkComplete, "Mark a task as complete"),
        (
            TaskCommand::MessageBrainAbout,
            "Message brain about a task",
        ),
        (TaskCommand::ToggleNotes, "Toggle a task's notes"),
        (TaskCommand::OpenLinks, "Open a task's link"),
        (TaskCommand::Remove, "Remove a task"),
        (TaskCommand::Reassign, "Reassign a task"),
        (TaskCommand::Defer(14), "Defer a task +14d"),
    ];
    for (command, expected) in cases {
        assert_eq!(
            label_of(&state, Command::Task(command)).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn a_highlighted_habit_leaves_tasks_only_rows_generic() {
    // A habit can't be deferred or removed through the task path, so those
    // rows stay generic (and will ask for a real task) rather than promising
    // to act on the habit.
    let state = palette(&with_task(task("H4", false, false, LinkKind::None)));

    assert_eq!(
        label_of(&state, Command::Task(TaskCommand::MarkComplete)).as_deref(),
        Some("Mark H4 as complete")
    );
    assert_eq!(
        label_of(&state, Command::Task(TaskCommand::Defer(1))).as_deref(),
        Some("Defer a task +1d")
    );
    assert_eq!(
        label_of(&state, Command::Task(TaskCommand::Remove)).as_deref(),
        Some("Remove a task")
    );
}

#[test]
fn the_notes_row_tracks_expansion_state() {
    let collapsed = palette(&with_task(task("T1", true, false, LinkKind::None)));
    let expanded = palette(&with_task(task("T1", true, true, LinkKind::None)));

    assert_eq!(
        label_of(&collapsed, Command::Task(TaskCommand::ToggleNotes)).as_deref(),
        Some("Expand T1 notes")
    );
    assert_eq!(
        label_of(&expanded, Command::Task(TaskCommand::ToggleNotes)).as_deref(),
        Some("Collapse T1 notes")
    );
}

#[test]
fn the_open_link_row_says_what_it_will_open() {
    let cases = [
        (LinkKind::SingleLinear, "Open T9 Linear link"),
        (LinkKind::SingleNotes, "Open link from T9's note"),
        (LinkKind::Multiple, "Open link attached to T9"),
        (LinkKind::None, "Open a link from T9"),
    ];
    for (links, expected) in cases {
        let state = palette(&with_task(task("T9", false, false, links)));
        assert_eq!(
            label_of(&state, Command::Task(TaskCommand::OpenLinks)).as_deref(),
            Some(expected),
            "{links:?}"
        );
    }
}
