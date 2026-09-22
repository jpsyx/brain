#[test]
fn the_palette_lists_the_same_commands_in_every_context() {
    // The invariant: the command *set* never moves with app state. A
    // highlighted task, a highlighted file, an open session, a running sync —
    // none of them add or remove a row. Only the wording changes.
    let empty = actions(&palette(&shared_workspace()));

    let with_a_task = actions(&palette(&with_task(task("T7", true, false, LinkKind::Multiple))));
    let with_an_entry = actions(&palette(&with_entry(entry("plan.md", true, true))));
    let with_sessions = actions(&palette(&PaletteContext {
        receiver_enabled: true,
        daily_triage_alert_disabled: true,
        show_hidden_files: true,
        ..shared_workspace()
    }));

    assert_eq!(empty, with_a_task);
    assert_eq!(empty, with_an_entry);
    assert_eq!(empty, with_sessions);
}

#[test]
fn task_commands_are_offered_with_no_task_highlighted() {
    let listed = actions(&palette(&shared_workspace()));

    for command in [
        TaskCommand::Start,
        TaskCommand::MarkComplete,
        TaskCommand::MessageBrainAbout,
        TaskCommand::ToggleNotes,
        TaskCommand::OpenLinks,
        TaskCommand::Remove,
        TaskCommand::Reassign,
        TaskCommand::Defer(1),
    ] {
        assert!(listed.contains(&Command::Task(command)), "{command:?}");
    }
}

#[test]
fn entry_commands_are_offered_from_the_tasks_view() {
    // Nothing is highlighted in the brain directory here; the rows are still
    // listed and will ask which file when run.
    let listed = actions(&palette(&shared_workspace()));

    for command in [
        EntryCommand::Open,
        EntryCommand::Reveal,
        EntryCommand::CopyFilePath,
        EntryCommand::CopyDirPath,
        EntryCommand::CreatePdf,
        EntryCommand::Delete,
    ] {
        assert!(listed.contains(&Command::Entry(command)), "{command:?}");
    }
}

#[test]
fn the_newly_reachable_commands_are_all_listed() {
    // Every shortcut that had no palette row before this change.
    let listed = actions(&palette(&shared_workspace()));

    for action in [
        GlobalAction::ShowShortcuts,
        GlobalAction::Quit,
        GlobalAction::NewConversation,
        GlobalAction::FocusBrainPanel,
        GlobalAction::FocusMainPanel,
        GlobalAction::CycleBrainTab(true),
        GlobalAction::CycleBrainTab(false),
        GlobalAction::ShowBrainSearch,
        GlobalAction::SearchTasks,
        GlobalAction::ClearTaskFilters,
        GlobalAction::ReloadTasks,
        GlobalAction::RefreshBrainDirectory,
        GlobalAction::ToggleLayout,
        GlobalAction::OpenFileExplorer,
        GlobalAction::ToggleHiddenFiles,
    ] {
        assert!(listed.contains(&Command::Global(action)), "{action:?}");
    }
}

#[test]
fn a_single_member_workspace_only_loses_the_assignment_controls() {
    let personal = actions(&palette(&PaletteContext::default()));
    let shared = actions(&palette(&shared_workspace()));

    let missing: Vec<Command> = shared
        .iter()
        .filter(|command| !personal.contains(command))
        .copied()
        .collect();
    assert_eq!(
        missing,
        vec![
            Command::Global(GlobalAction::AddTask),
            Command::Task(TaskCommand::Reassign),
            Command::Global(GlobalAction::ChooseAssigneeFilter),
        ]
    );
}

#[test]
fn direct_shortcuts_are_advertised_next_to_their_rows() {
    let cases = [
        (Command::Task(TaskCommand::MarkComplete), Some("^D")),
        (Command::Task(TaskCommand::OpenLinks), Some("^O")),
        (Command::Task(TaskCommand::Remove), Some("^⌫")),
        (Command::Entry(EntryCommand::CreatePdf), Some("^G")),
        (Command::Entry(EntryCommand::Delete), Some("^D")),
        (Command::Global(GlobalAction::ShowShortcuts), Some("⌥S")),
        (Command::Global(GlobalAction::OpenFileExplorer), Some("^E")),
        (Command::Global(GlobalAction::ToggleHiddenFiles), Some(".")),
        (Command::Entry(EntryCommand::Explore), Some("⌥↵")),
        (Command::Global(GlobalAction::Quit), Some("^Q")),
        (Command::Global(GlobalAction::SyncBrainNow), None),
        (Command::Task(TaskCommand::Start), None),
    ];
    for (command, expected) in cases {
        assert_eq!(shortcut_for(command), expected, "{command:?}");
    }
}
