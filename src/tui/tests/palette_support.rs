// Tests for the unified command palette: the parent-set invariant, contextual
// vs. generic wording, the session rows, and the task actions modal.

use crate::skill_session::SkillSessionKey;
use crate::tasks::task::AssignmentUiMode;
use crate::tui::action::GlobalAction;
use crate::tui::links::LinkKind;
use crate::tui::model::SessionTabId;
use crate::tui::palette::{
    Command, CommandPaletteState, EntryCommand, EntryContext, PaletteContext, TaskCommand,
    TaskContext, shortcut_for,
};
use crate::tui::state::SessionPaletteEntry;

/// A workspace with every capability, so the assignment controls are offered.
fn shared_workspace() -> PaletteContext {
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

fn with_task(task: TaskContext) -> PaletteContext {
    PaletteContext {
        task: Some(task),
        ..shared_workspace()
    }
}

fn task(id: &str, has_notes: bool, notes_expanded: bool, links: LinkKind) -> TaskContext {
    TaskContext {
        id: id.to_owned(),
        label: "a task".to_owned(),
        is_habit: id.starts_with('H'),
        has_notes,
        notes_expanded,
        links,
    }
}

fn entry(filename: &str, is_file: bool, is_markdown: bool) -> EntryContext {
    EntryContext {
        filename: filename.to_owned(),
        dir_reldisplay: "projects/atlas".to_owned(),
        is_file,
        is_markdown,
    }
}

fn with_entry(entry: EntryContext) -> PaletteContext {
    PaletteContext {
        entry: Some(entry),
        ..shared_workspace()
    }
}

fn palette(context: &PaletteContext) -> CommandPaletteState {
    CommandPaletteState::new(context)
}

fn actions(state: &CommandPaletteState) -> Vec<Command> {
    state.visible().iter().map(|row| row.action).collect()
}

fn label_of(state: &CommandPaletteState, command: Command) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|row| row.action == command)
        .map(|row| row.label.clone())
}
