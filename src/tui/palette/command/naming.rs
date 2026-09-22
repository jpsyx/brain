//! How each [`Command`] is worded in a palette row.
//!
//! A row reads one of three ways:
//!
//! - **Named** — the command's target is already in context, so the row says
//!   exactly what it will act on ("Mark T123 as complete", "Delete 'plan.md'").
//! - **Generic** — the target is missing, so the row says what *kind* of thing
//!   it will act on ("Mark a task as complete"). Running it asks for the
//!   target first.
//! - **Bare** — the task actions modal, whose title already names the task, so
//!   its rows drop the ID ("Mark as complete").

use crate::entry::Bucket;
use crate::state::PanelSide;
use crate::tasks::view::View;
use crate::tui::action::GlobalAction;
use crate::tui::links::LinkKind;

use super::labels::{
    copy_dir_path_label, copy_file_path_label, create_pdf_label, delete_label, explore_label,
    open_dir_label, open_file_label, reveal_dir_label,
};
use super::{Command, EntryCommand, TaskCommand};
use crate::tui::palette::context::{EntryContext, PaletteContext, TaskContext};

/// Which wording a surface wants for its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabelStyle {
    /// The global command palette: name the target when there is one.
    Palette,
    /// The task actions modal: the title already names the task.
    TaskActions,
}

/// The row label for `command` under `context`.
pub(crate) fn label_for(command: Command, context: &PaletteContext, style: LabelStyle) -> String {
    match command {
        Command::Global(action) => global_label(action, context),
        Command::Task(task) => match (style, context.task_target(task)) {
            (LabelStyle::TaskActions, _) => bare_task_label(task, context.task.as_ref()),
            (LabelStyle::Palette, Some(target)) => named_task_label(task, target),
            (LabelStyle::Palette, None) => generic_task_label(task),
        },
        Command::Entry(entry) => context.entry_target(entry).map_or_else(
            || generic_entry_label(entry).to_owned(),
            |target| named_entry_label(entry, target),
        ),
    }
}

fn global_label(action: GlobalAction, context: &PaletteContext) -> String {
    match action {
        // The four toggles name the action that will happen next, mirroring
        // the Start/Stop-style rows elsewhere.
        GlobalAction::ToggleReceiver => if context.receiver_enabled {
            "Disable receiver"
        } else {
            "Enable receiver"
        }
        .to_owned(),
        GlobalAction::ToggleDailyTriageAlert => if context.daily_triage_alert_disabled {
            "Enable daily triage alert"
        } else {
            "Disable daily triage alert"
        }
        .to_owned(),
        GlobalAction::ToggleHiddenFiles => if context.show_hidden_files {
            "Hide hidden files"
        } else {
            "Show hidden files"
        }
        .to_owned(),
        GlobalAction::ToggleLayout => layout_choice_label(context.panel_side).to_owned(),
        other => static_global_label(other).to_owned(),
    }
}

/// The label for the layout-toggle row: it names the direction the panel would
/// move, i.e. the *opposite* of where it sits now.
#[must_use]
pub(crate) const fn layout_choice_label(side: PanelSide) -> &'static str {
    match side {
        PanelSide::Right => "Move brain panel to the left",
        PanelSide::Left => "Move brain panel to the right",
    }
}

/// The fixed wording for every global action whose label doesn't move with
/// state. The two dynamic rows (`ShowSessionTab`, `RunSkillSession`) carry
/// per-session labels supplied by the catalog; the fallbacks here are only
/// reached if one is ever listed without them.
fn static_global_label(action: GlobalAction) -> &'static str {
    match action {
        GlobalAction::MessageBrain => "Message brain",
        GlobalAction::StartManualSession => "Start new brain session",
        GlobalAction::RenameSession => "Rename session",
        GlobalAction::CloseSession => "Close a brain session",
        GlobalAction::NewConversation => "Start a new conversation in this session",
        GlobalAction::ShowMainBrainSession => "Show main brain session",
        GlobalAction::ShowSessionTab(_) => "Show session",
        GlobalAction::CycleBrainTab(true) => "Next brain tab",
        GlobalAction::CycleBrainTab(false) => "Previous brain tab",
        GlobalAction::FocusBrainPanel => "Focus the brain panel",
        GlobalAction::FocusMainPanel => "Focus the main view",
        GlobalAction::ShowTasks => "Show the tasks view",
        GlobalAction::ShowBrainSearch => "Show the brain directory search",
        GlobalAction::OpenFileExplorer => "Open the file explorer at the brain root",
        GlobalAction::ShowReceiverServerStatus => "Show receiver server status",
        GlobalAction::ShowReceiverServerLogs => "Show receiver logs",
        GlobalAction::ShowBrainLogs => "Show brain logs",
        GlobalAction::OpenHabits => "Open habits in browser",
        GlobalAction::SyncBrainNow => "Sync brain now",
        GlobalAction::ShowSyncStatus => "Show sync status",
        GlobalAction::OpenAgenda => "Open today's agenda",
        GlobalAction::RunSkillSession(_) => "Run skill session",
        GlobalAction::AddTask => "Add task",
        GlobalAction::ChooseAssigneeFilter => "Filter by assignee",
        GlobalAction::ClearTaskFilters => "Clear the task filters",
        GlobalAction::SearchTasks => "Search tasks",
        GlobalAction::ReloadTasks => "Reload tasks and habits",
        GlobalAction::ShowTaskView(view) => task_view_label(view),
        GlobalAction::RefreshBrainDirectory => "Refresh the brain directory",
        GlobalAction::SearchBucket(bucket) => bucket_search_label(bucket),
        GlobalAction::SearchEverything => "Global search",
        GlobalAction::ShowShortcuts => "Show keyboard shortcuts",
        GlobalAction::Quit => "Quit brain",
        // Resolved by `global_label` before reaching here.
        GlobalAction::ToggleReceiver
        | GlobalAction::ToggleDailyTriageAlert
        | GlobalAction::ToggleHiddenFiles
        | GlobalAction::ToggleLayout => "",
    }
}

const fn task_view_label(view: View) -> &'static str {
    match view {
        View::Today => "Show today's tasks",
        View::Mit => "Show MIT tasks",
        View::PastDue => "Show past-due tasks",
        View::Week => "Show this week's tasks",
        View::Habits => "Show habits",
        View::Backlog => "Show the backlog",
        View::All => "Show all tasks",
    }
}

const fn bucket_search_label(bucket: Bucket) -> &'static str {
    match bucket {
        Bucket::Capture => "Search capture",
        Bucket::Projects => "Search projects",
        Bucket::Areas => "Search areas",
        Bucket::Resources => "Search resources",
        Bucket::Archive => "Search archive",
    }
}

fn named_task_label(command: TaskCommand, target: &TaskContext) -> String {
    let id = &target.id;
    match command {
        TaskCommand::Start => format!("Start {id}"),
        TaskCommand::MarkComplete => format!("Mark {id} as complete"),
        TaskCommand::MessageBrainAbout => format!("Message brain about {id}"),
        TaskCommand::ToggleNotes => format!("{} {id} notes", notes_verb(target.notes_expanded)),
        TaskCommand::OpenLinks => match target.links {
            LinkKind::SingleLinear => format!("Open {id} Linear link"),
            LinkKind::SingleNotes => format!("Open link from {id}'s note"),
            LinkKind::Multiple => format!("Open link attached to {id}"),
            LinkKind::None => format!("Open a link from {id}"),
        },
        TaskCommand::Remove => format!("Remove task {id}"),
        TaskCommand::Reassign => format!("Reassign {id}"),
        TaskCommand::Defer(days) => format!("Defer {id} +{days}d"),
    }
}

fn generic_task_label(command: TaskCommand) -> String {
    match command {
        TaskCommand::Start => "Start a task".to_owned(),
        TaskCommand::MarkComplete => "Mark a task as complete".to_owned(),
        TaskCommand::MessageBrainAbout => "Message brain about a task".to_owned(),
        TaskCommand::ToggleNotes => "Toggle a task's notes".to_owned(),
        TaskCommand::OpenLinks => "Open a task's link".to_owned(),
        TaskCommand::Remove => "Remove a task".to_owned(),
        TaskCommand::Reassign => "Reassign a task".to_owned(),
        TaskCommand::Defer(days) => format!("Defer a task +{days}d"),
    }
}

fn bare_task_label(command: TaskCommand, target: Option<&TaskContext>) -> String {
    match command {
        TaskCommand::Start => "Start this task".to_owned(),
        TaskCommand::MarkComplete => "Mark as complete".to_owned(),
        TaskCommand::MessageBrainAbout => "Message brain about this task".to_owned(),
        TaskCommand::ToggleNotes => format!(
            "{} notes",
            notes_verb(target.is_some_and(|task| task.notes_expanded))
        ),
        TaskCommand::OpenLinks => match target.map_or(LinkKind::None, |task| task.links) {
            LinkKind::SingleLinear => "Open Linear link".to_owned(),
            LinkKind::SingleNotes => "Open link from note".to_owned(),
            LinkKind::Multiple => "Open attached link".to_owned(),
            LinkKind::None => "Open link".to_owned(),
        },
        TaskCommand::Remove => "Remove this task".to_owned(),
        TaskCommand::Reassign => "Reassign this task".to_owned(),
        TaskCommand::Defer(days) => format!("Defer +{days}d"),
    }
}

const fn notes_verb(expanded: bool) -> &'static str {
    if expanded { "Collapse" } else { "Expand" }
}

fn named_entry_label(command: EntryCommand, target: &EntryContext) -> String {
    match command {
        EntryCommand::Open if target.is_file => open_file_label(&target.filename),
        EntryCommand::Open => open_dir_label(&target.dir_reldisplay),
        EntryCommand::Reveal => reveal_dir_label(&target.dir_reldisplay),
        EntryCommand::Explore => explore_label(&target.filename),
        EntryCommand::CopyFilePath => copy_file_path_label(&target.filename),
        EntryCommand::CopyDirPath => copy_dir_path_label(&target.dir_reldisplay),
        EntryCommand::CreatePdf => create_pdf_label(&target.filename),
        EntryCommand::Delete => delete_label(&target.filename),
    }
}

const fn generic_entry_label(command: EntryCommand) -> &'static str {
    match command {
        EntryCommand::Open => "Open a file or directory",
        EntryCommand::Reveal => "Reveal a directory in Finder",
        EntryCommand::Explore => "Open the file explorer on a file or directory",
        EntryCommand::CopyFilePath => "Copy a file's path",
        EntryCommand::CopyDirPath => "Copy a directory's path",
        EntryCommand::CreatePdf => "Create a PDF from a markdown file",
        EntryCommand::Delete => "Delete a file or directory",
    }
}
