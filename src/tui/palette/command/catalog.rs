//! The ordered command table and the two row builders that read it.
//!
//! Order here is the order shown in the palette. The table is *complete*: the
//! palette lists every row in it no matter which main view is showing or what
//! is highlighted. The only rows that can drop out are the ones a workspace
//! genuinely does not have — the assignment controls of a single-member
//! workspace — which is a capability of the workspace, not a state of the app.

use crate::entry::Bucket;
use crate::tasks::view::View;
use crate::tui::action::GlobalAction;
use crate::tui::palette::context::PaletteContext;
use crate::tui::palette::model::PaletteRow;
use crate::tui::palette::sessions::session_rows;

use super::naming::{LabelStyle, label_for};
use super::{Command, EntryCommand, TaskCommand, shortcut_for};

use Command::{Entry, Global, Task};

/// Every command brain declares, in display order. Per-session rows are
/// injected after [`GlobalAction::ShowMainBrainSession`].
const COMMANDS: &[Command] = &[
    // --- brain sessions ---
    Global(GlobalAction::MessageBrain),
    Global(GlobalAction::StartManualSession),
    Global(GlobalAction::RenameSession),
    Global(GlobalAction::CloseSession),
    Global(GlobalAction::NewConversation),
    Global(GlobalAction::ShowMainBrainSession),
    // --- tasks ---
    Global(GlobalAction::AddTask),
    Task(TaskCommand::Start),
    Task(TaskCommand::MarkComplete),
    Task(TaskCommand::MessageBrainAbout),
    Task(TaskCommand::ToggleNotes),
    Task(TaskCommand::OpenLinks),
    Task(TaskCommand::Remove),
    Task(TaskCommand::Reassign),
    Task(TaskCommand::Defer(1)),
    Task(TaskCommand::Defer(7)),
    Task(TaskCommand::Defer(14)),
    Global(GlobalAction::ChooseAssigneeFilter),
    Global(GlobalAction::SearchTasks),
    Global(GlobalAction::ClearTaskFilters),
    Global(GlobalAction::ReloadTasks),
    Global(GlobalAction::ShowTaskView(View::Today)),
    Global(GlobalAction::ShowTaskView(View::Mit)),
    Global(GlobalAction::ShowTaskView(View::PastDue)),
    Global(GlobalAction::ShowTaskView(View::Week)),
    Global(GlobalAction::ShowTaskView(View::Habits)),
    Global(GlobalAction::ShowTaskView(View::Backlog)),
    Global(GlobalAction::ShowTaskView(View::All)),
    // --- brain directory ---
    Entry(EntryCommand::Open),
    Entry(EntryCommand::Reveal),
    Entry(EntryCommand::Explore),
    Global(GlobalAction::OpenFileExplorer),
    Global(GlobalAction::ToggleHiddenFiles),
    Entry(EntryCommand::CopyFilePath),
    Entry(EntryCommand::CopyDirPath),
    Entry(EntryCommand::CreatePdf),
    Entry(EntryCommand::Delete),
    Global(GlobalAction::SearchBucket(Bucket::Capture)),
    Global(GlobalAction::SearchBucket(Bucket::Projects)),
    Global(GlobalAction::SearchBucket(Bucket::Areas)),
    Global(GlobalAction::SearchBucket(Bucket::Resources)),
    Global(GlobalAction::SearchBucket(Bucket::Archive)),
    Global(GlobalAction::SearchEverything),
    Global(GlobalAction::RefreshBrainDirectory),
    // --- views and layout ---
    Global(GlobalAction::ShowTasks),
    Global(GlobalAction::ShowBrainSearch),
    Global(GlobalAction::ShowBrainLogs),
    Global(GlobalAction::ToggleLayout),
    Global(GlobalAction::FocusBrainPanel),
    Global(GlobalAction::FocusMainPanel),
    Global(GlobalAction::CycleBrainTab(true)),
    Global(GlobalAction::CycleBrainTab(false)),
    // --- workspace ---
    Global(GlobalAction::OpenAgenda),
    Global(GlobalAction::OpenHabits),
    Global(GlobalAction::SyncBrainNow),
    Global(GlobalAction::ShowSyncStatus),
    Global(GlobalAction::ToggleReceiver),
    Global(GlobalAction::ShowReceiverServerStatus),
    Global(GlobalAction::ShowReceiverServerLogs),
    Global(GlobalAction::ToggleDailyTriageAlert),
    Global(GlobalAction::ShowShortcuts),
    Global(GlobalAction::Quit),
];

/// Whether this workspace has the command at all.
///
/// The assignment controls are meaningless in a workspace with one member, so
/// they stay out of its palette. This is the *only* reason a declared command
/// is ever missing, and it turns on a workspace capability rather than on
/// which view happens to be showing.
fn workspace_offers(command: Command, context: &PaletteContext) -> bool {
    match command {
        Global(GlobalAction::AddTask) => context.assignment_mode.show_create_control,
        Global(GlobalAction::ChooseAssigneeFilter) => context.assignment_mode.show_filter,
        Task(TaskCommand::Reassign) => context.assignment_mode.show_reassign_control,
        _ => true,
    }
}

/// Every row of the global command palette, in canonical order and *before*
/// the text filter, each carrying the stable 1-based number the palette shows
/// (so the digit a user types always points at the same row).
pub(crate) fn catalog_rows(context: &PaletteContext) -> Vec<PaletteRow<Command>> {
    let mut rows: Vec<PaletteRow<Command>> = Vec::new();
    for command in COMMANDS
        .iter()
        .copied()
        .filter(|command| workspace_offers(*command, context))
    {
        push_row(&mut rows, label_for(command, context, LabelStyle::Palette), command);
        if command == Global(GlobalAction::ShowMainBrainSession) {
            for (label, action) in
                session_rows(&context.runnable_skill_sessions, &context.user_sessions)
            {
                push_row(&mut rows, label, Global(action));
            }
        }
    }
    rows
}

/// The task actions modal's rows: the task-scoped slice of the same table,
/// bound to the entry the user pressed Enter on. Habit-incompatible commands
/// drop out, because that modal is already committed to one specific row.
pub(crate) fn task_action_rows(context: &PaletteContext) -> Vec<PaletteRow<Command>> {
    let habit = context.task.as_ref().is_some_and(|task| task.is_habit);
    let mut rows: Vec<PaletteRow<Command>> = Vec::new();
    for command in COMMANDS
        .iter()
        .copied()
        .filter(|command| workspace_offers(*command, context))
    {
        let Task(task) = command else { continue };
        if habit && !task.works_on_habits() {
            continue;
        }
        push_row(
            &mut rows,
            label_for(command, context, LabelStyle::TaskActions),
            command,
        );
    }
    rows
}

/// Append one row, numbering it by its position. The number is what the
/// palette shows and what a typed digit selects, so it must stay 1-based and
/// gapless.
fn push_row(rows: &mut Vec<PaletteRow<Command>>, label: String, command: Command) {
    let mut row = PaletteRow::new(label, command, shortcut_for(command));
    row.number = rows.len() + 1;
    rows.push(row);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::task::AssignmentUiMode;
    use crate::tui::palette::context::TaskContext;
    use crate::tui::links::LinkKind;

    fn shared() -> PaletteContext {
        PaletteContext {
            assignment_mode: AssignmentUiMode {
                show_in_detail: true,
                show_create_control: true,
                show_reassign_control: true,
                show_filter: true,
            },
            ..PaletteContext::default()
        }
    }

    #[test]
    fn every_declared_command_is_listed_with_no_target_in_context() {
        // The whole point of the invariant: an empty context still offers the
        // full table, so no action is unreachable from the palette.
        let context = shared();
        let listed: Vec<Command> = catalog_rows(&context)
            .into_iter()
            .map(|row| row.action)
            .collect();

        for command in COMMANDS {
            assert!(listed.contains(command), "{command:?} is missing");
        }
    }

    #[test]
    fn exploring_an_entry_is_a_listed_command() {
        // The tree sub-view has to be reachable without the keystroke, from
        // any view, which is what the parent-set invariant guarantees.
        assert!(
            catalog_rows(&shared())
                .into_iter()
                .any(|row| row.action == Entry(EntryCommand::Explore))
        );
    }

    #[test]
    fn the_command_set_does_not_change_when_a_task_is_highlighted() {
        let without = shared();
        let with = PaletteContext {
            task: Some(TaskContext {
                id: "T1".into(),
                label: "row".into(),
                is_habit: false,
                has_notes: false,
                notes_expanded: false,
                links: LinkKind::None,
            }),
            ..shared()
        };

        let commands = |context: &PaletteContext| -> Vec<Command> {
            catalog_rows(context).into_iter().map(|row| row.action).collect()
        };
        assert_eq!(commands(&without), commands(&with));
    }

    #[test]
    fn rows_are_numbered_from_one_without_gaps() {
        let rows = catalog_rows(&shared());
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.number, index + 1);
        }
    }

    #[test]
    fn a_single_member_workspace_drops_only_the_assignment_controls() {
        let personal = catalog_rows(&PaletteContext::default());
        let listed: Vec<Command> = personal.into_iter().map(|row| row.action).collect();

        assert!(!listed.contains(&Global(GlobalAction::AddTask)));
        assert!(!listed.contains(&Global(GlobalAction::ChooseAssigneeFilter)));
        assert!(!listed.contains(&Task(TaskCommand::Reassign)));
        assert!(listed.contains(&Task(TaskCommand::MarkComplete)));
    }

    #[test]
    fn the_task_actions_modal_lists_only_task_commands() {
        let rows = task_action_rows(&shared());
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| matches!(row.action, Task(_))));
    }

    #[test]
    fn the_task_actions_modal_hides_tasks_only_commands_for_a_habit() {
        let habit = PaletteContext {
            task: Some(TaskContext {
                id: "H1".into(),
                label: "habit".into(),
                is_habit: true,
                has_notes: false,
                notes_expanded: false,
                links: LinkKind::None,
            }),
            ..shared()
        };
        let listed: Vec<Command> = task_action_rows(&habit)
            .into_iter()
            .map(|row| row.action)
            .collect();

        assert!(!listed.contains(&Task(TaskCommand::Defer(1))));
        assert!(!listed.contains(&Task(TaskCommand::Remove)));
        assert!(listed.contains(&Task(TaskCommand::MarkComplete)));
    }
}
