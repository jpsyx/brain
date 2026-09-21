//! The unified command vocabulary.
//!
//! Every action the shell can perform is one [`Command`]. The command palette
//! lists **all** of them regardless of which main view is showing, so a
//! command's *scope* no longer decides whether it appears — it only decides
//! what the command needs before it can run:
//!
//! - [`Command::Global`] needs nothing.
//! - [`Command::Task`] needs a task (or habit).
//! - [`Command::Entry`] needs a brain-directory file or directory.
//!
//! When the current context already supplies that target (a highlighted task
//! in the tasks view, a highlighted entry in the brain-directory view) the row
//! names it — "Mark T123 as complete". When it doesn't, the row reads
//! generically — "Mark a task as complete" — and running it opens a picker for
//! the missing target first. See `target` for those picker states.

mod catalog;
mod labels;
mod naming;

pub(crate) use catalog::{catalog_rows, task_action_rows};

use crate::tui::action::GlobalAction;

/// One user-visible action, in the single vocabulary shared by every palette
/// surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Runs against the app itself; no target needed.
    Global(GlobalAction),
    /// Runs against one task or habit.
    Task(TaskCommand),
    /// Runs against one brain-directory file or directory.
    Entry(EntryCommand),
}

/// An action that operates on a single task or habit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskCommand {
    /// Spawn the brain panel with a "let's start this task" prompt that asks
    /// the agent to gather context and propose first steps.
    Start,
    /// Complete the row natively, then reload `tasks.csv` + `habits.csv`.
    MarkComplete,
    /// Like [`GlobalAction::MessageBrain`], but the entered text is prefixed
    /// with "This message is about <ID>:" so the agent knows the subject.
    MessageBrainAbout,
    /// Toggle the entry's notes between a single-line preview and the full
    /// markdown-rendered body.
    ToggleNotes,
    /// Open the entry's link(s): its Linear issue and/or any URLs in its
    /// notes. A single link opens directly; several raise the picker.
    OpenLinks,
    /// Spawn the brain panel with a "remove this task" prompt.
    Remove,
    /// Ask the brain agent to reassign the task or habit.
    Reassign,
    /// Spawn the brain panel with a "defer this task by N days" prompt.
    Defer(u32),
}

impl TaskCommand {
    /// Whether the command reads sensibly for a habit. Defer and remove have
    /// habit-specific flows elsewhere, so they stay tasks-only: a habit target
    /// is rejected rather than silently sending the wrong instruction.
    pub(crate) const fn works_on_habits(self) -> bool {
        match self {
            Self::MarkComplete
            | Self::MessageBrainAbout
            | Self::ToggleNotes
            | Self::OpenLinks
            | Self::Reassign => true,
            Self::Start | Self::Remove | Self::Defer(_) => false,
        }
    }

    pub(crate) const fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::MarkComplete => Some("^D"),
            Self::MessageBrainAbout => Some("^⇧M"),
            Self::ToggleNotes => Some("l"),
            Self::OpenLinks => Some("^O"),
            Self::Remove => Some("^⌫"),
            Self::Start | Self::Reassign | Self::Defer(_) => None,
        }
    }

    /// What the picker's title calls this command while the user chooses which
    /// task to run it on.
    pub(crate) fn picker_title(self) -> String {
        match self {
            Self::Start => "Start which task?".to_owned(),
            Self::MarkComplete => "Mark which entry complete?".to_owned(),
            Self::MessageBrainAbout => "Message brain about which entry?".to_owned(),
            Self::ToggleNotes => "Toggle whose notes?".to_owned(),
            Self::OpenLinks => "Open whose link?".to_owned(),
            Self::Remove => "Remove which task?".to_owned(),
            Self::Reassign => "Reassign which entry?".to_owned(),
            Self::Defer(days) => format!("Defer which task by {days}d?"),
        }
    }
}

/// An action that operates on a single brain-directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryCommand {
    /// Open the highlighted entry in place (text → editor tab, blob → system
    /// open, directory → Finder).
    Open,
    /// Reveal the entry's directory in Finder. A file resolves to its parent.
    Reveal,
    /// Copy the entry's absolute file path.
    CopyFilePath,
    /// Copy the entry's directory path.
    CopyDirPath,
    /// Convert the highlighted markdown file to a colocated PDF.
    CreatePdf,
    /// Move the entry to the Trash (behind a red confirmation).
    Delete,
}

/// What an [`EntryCommand`] needs of its target before it can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryRequirement {
    /// Any file or directory.
    Any,
    /// A file (a directory has nothing to open or copy a file path from).
    File,
    /// A markdown file.
    Markdown,
}

impl EntryCommand {
    pub(crate) const fn requirement(self) -> EntryRequirement {
        match self {
            Self::Open | Self::Reveal | Self::CopyDirPath | Self::Delete => EntryRequirement::Any,
            Self::CopyFilePath => EntryRequirement::File,
            Self::CreatePdf => EntryRequirement::Markdown,
        }
    }

    pub(crate) const fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Open => Some("↵"),
            Self::Reveal => Some("^↵"),
            Self::CreatePdf => Some("^G"),
            Self::Delete => Some("^D"),
            Self::CopyFilePath | Self::CopyDirPath => None,
        }
    }

    /// What the picker's title calls this command while the user chooses which
    /// entry to run it on.
    pub(crate) const fn picker_title(self) -> &'static str {
        match self {
            Self::Open => "Open which entry?",
            Self::Reveal => "Reveal which directory?",
            Self::CopyFilePath => "Copy which file's path?",
            Self::CopyDirPath => "Copy which directory's path?",
            Self::CreatePdf => "Create a PDF from which markdown file?",
            Self::Delete => "Delete which entry?",
        }
    }

    /// The rejection shown when the picked entry can't satisfy this command.
    pub(crate) const fn requirement_error(self) -> &'static str {
        match self.requirement() {
            EntryRequirement::Any => "that entry cannot be used here",
            EntryRequirement::File => "that is a directory, not a file",
            EntryRequirement::Markdown => "that is not a markdown file",
        }
    }
}

/// Direct keystroke that bypasses the palette for a given command, rendered as
/// a dim `[…]` annotation next to the palette label. `None` when a command has
/// no direct shortcut.
pub(crate) const fn shortcut_for(command: Command) -> Option<&'static str> {
    match command {
        Command::Global(action) => action.shortcut(),
        Command::Task(task) => task.shortcut(),
        Command::Entry(entry) => entry.shortcut(),
    }
}
