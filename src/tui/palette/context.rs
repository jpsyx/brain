//! What the palette knows about the world when it opens.
//!
//! The context never decides *which* commands are listed — every command is
//! always listed. It decides how each row is **worded** and whether running it
//! needs a target picker first: a command whose target is already in context
//! runs straight away, and one whose target is missing asks for it.

use crate::skill_session::SkillSessionKey;
use crate::state::PanelSide;
use crate::tasks::task::AssignmentUiMode;
use crate::tui::links::LinkKind;
use crate::tui::state::SessionPaletteEntry;

use super::command::{EntryCommand, EntryRequirement, TaskCommand};

/// The task or habit the palette can act on without asking.
///
/// Only populated when the tasks view is the showing main view *and* it has a
/// highlighted row: a stale selection behind another main view is not
/// something the user is pointing at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskContext {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) is_habit: bool,
    pub(crate) has_notes: bool,
    pub(crate) notes_expanded: bool,
    pub(crate) links: LinkKind,
}

/// The brain-directory entry the palette can act on without asking.
///
/// Only populated when the brain-directory view is showing and has a
/// highlighted entry, for the same reason as [`TaskContext`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryContext {
    /// The highlighted entry's file name, for the contextual row label.
    pub(crate) filename: String,
    /// The entry's directory as a bucket-relative display path.
    pub(crate) dir_reldisplay: String,
    pub(crate) is_file: bool,
    pub(crate) is_markdown: bool,
}

impl EntryContext {
    pub(crate) const fn satisfies(&self, requirement: EntryRequirement) -> bool {
        match requirement {
            EntryRequirement::Any => true,
            EntryRequirement::File => self.is_file,
            EntryRequirement::Markdown => self.is_markdown,
        }
    }
}

/// Everything the palette reads at open time.
#[derive(Clone, Debug)]
pub(crate) struct PaletteContext {
    pub(crate) task: Option<TaskContext>,
    pub(crate) entry: Option<EntryContext>,
    pub(crate) receiver_enabled: bool,
    pub(crate) daily_triage_alert_disabled: bool,
    /// Whether the tree sub-view is currently showing dotted names, so the
    /// toggle row can name the flip that will happen next.
    pub(crate) show_hidden_files: bool,
    pub(crate) panel_side: PanelSide,
    pub(crate) assignment_mode: AssignmentUiMode,
    /// The skill sessions that can be started right now, each with the
    /// `command_label` its row shows. A session already running is absent,
    /// which is what stops a user starting the same one twice.
    pub(crate) runnable_skill_sessions: Vec<(SkillSessionKey, String)>,
    /// Open manual and skill tabs captured with stable identities.
    pub(crate) user_sessions: Vec<SessionPaletteEntry>,
}

impl Default for PaletteContext {
    fn default() -> Self {
        Self {
            task: None,
            entry: None,
            receiver_enabled: false,
            daily_triage_alert_disabled: false,
            show_hidden_files: false,
            panel_side: PanelSide::DEFAULT,
            assignment_mode: AssignmentUiMode {
                show_in_detail: false,
                show_create_control: false,
                show_reassign_control: false,
                show_filter: false,
            },
            runnable_skill_sessions: Vec::new(),
            user_sessions: Vec::new(),
        }
    }
}

impl PaletteContext {
    /// The in-context task this command can run on straight away, if any. A
    /// habit is not a valid target for a tasks-only command, so those fall
    /// through to the picker instead of acting on the wrong row.
    pub(crate) fn task_target(&self, command: TaskCommand) -> Option<&TaskContext> {
        self.task
            .as_ref()
            .filter(|task| command.works_on_habits() || !task.is_habit)
    }

    /// The in-context entry this command can run on straight away, if any.
    pub(crate) fn entry_target(&self, command: EntryCommand) -> Option<&EntryContext> {
        self.entry
            .as_ref()
            .filter(|entry| entry.satisfies(command.requirement()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(is_habit: bool) -> TaskContext {
        TaskContext {
            id: if is_habit { "H1".into() } else { "T1".into() },
            label: "row".into(),
            is_habit,
            has_notes: false,
            notes_expanded: false,
            links: LinkKind::None,
        }
    }

    fn entry(is_file: bool, is_markdown: bool) -> EntryContext {
        EntryContext {
            filename: "plan.md".into(),
            dir_reldisplay: "projects/atlas".into(),
            is_file,
            is_markdown,
        }
    }

    #[test]
    fn a_habit_is_not_a_target_for_a_tasks_only_command() {
        let context = PaletteContext {
            task: Some(task(true)),
            ..PaletteContext::default()
        };

        assert!(context.task_target(TaskCommand::Defer(1)).is_none());
        assert!(context.task_target(TaskCommand::Remove).is_none());
        assert!(context.task_target(TaskCommand::Start).is_none());
        assert!(context.task_target(TaskCommand::MarkComplete).is_some());
    }

    #[test]
    fn a_task_is_a_target_for_every_task_command() {
        let context = PaletteContext {
            task: Some(task(false)),
            ..PaletteContext::default()
        };

        for command in [
            TaskCommand::Start,
            TaskCommand::MarkComplete,
            TaskCommand::MessageBrainAbout,
            TaskCommand::ToggleNotes,
            TaskCommand::OpenLinks,
            TaskCommand::Remove,
            TaskCommand::Reassign,
            TaskCommand::Defer(7),
        ] {
            assert!(context.task_target(command).is_some(), "{command:?}");
        }
    }

    #[test]
    fn entry_requirements_gate_the_in_context_target() {
        let directory = PaletteContext {
            entry: Some(entry(false, false)),
            ..PaletteContext::default()
        };
        let plain_file = PaletteContext {
            entry: Some(entry(true, false)),
            ..PaletteContext::default()
        };
        let markdown = PaletteContext {
            entry: Some(entry(true, true)),
            ..PaletteContext::default()
        };

        assert!(directory.entry_target(EntryCommand::Reveal).is_some());
        assert!(directory.entry_target(EntryCommand::CopyFilePath).is_none());
        assert!(directory.entry_target(EntryCommand::CreatePdf).is_none());

        assert!(plain_file.entry_target(EntryCommand::CopyFilePath).is_some());
        assert!(plain_file.entry_target(EntryCommand::CreatePdf).is_none());

        assert!(markdown.entry_target(EntryCommand::CreatePdf).is_some());
    }

    #[test]
    fn an_empty_context_offers_no_target_at_all() {
        let context = PaletteContext::default();

        assert!(context.task_target(TaskCommand::MarkComplete).is_none());
        assert!(context.entry_target(EntryCommand::Open).is_none());
    }
}
