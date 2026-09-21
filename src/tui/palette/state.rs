//! The two command surfaces built on the shared filterable palette: the
//! global command palette and the per-task actions modal.

use crossterm::event::KeyEvent;

use super::command::{Command, catalog_rows, task_action_rows};
use super::context::PaletteContext;
use super::model::{CommandPalette, PaletteControls, PaletteRow, PaletteStep};

/// The global command palette (`Ctrl+P`) and the task actions modal (`Enter`
/// on a task) share this state; `task_actions` says which one is open.
pub(crate) struct CommandPaletteState {
    palette: CommandPalette<Command>,
    task_actions: bool,
}

impl CommandPaletteState {
    /// The global command palette: every command brain declares, worded for
    /// the targets `context` supplies.
    pub(crate) fn new(context: &PaletteContext) -> Self {
        let rows = catalog_rows(context);
        Self {
            palette: palette_over("Command palette", None, rows),
            task_actions: false,
        }
    }

    /// The task actions modal. The caller must put the pressed-on task in
    /// `context.task`; its ID titles the modal and its name is the subtitle.
    pub(crate) fn new_task_actions(context: &PaletteContext) -> Self {
        let rows = task_action_rows(context);
        let (title, subtitle) = context.task.as_ref().map_or_else(
            || ("Task actions".to_owned(), None),
            |task| {
                (
                    format!("Task {} actions", task.id),
                    Some(task.label.clone()),
                )
            },
        );
        Self {
            palette: palette_over(title, subtitle, rows),
            task_actions: true,
        }
    }

    pub(crate) const fn task_actions_modal(&self) -> bool {
        self.task_actions
    }

    pub(crate) fn title(&self) -> &str {
        self.palette.title()
    }

    pub(crate) fn subtitle(&self) -> Option<&str> {
        self.palette.subtitle()
    }

    pub(crate) fn query(&self) -> &str {
        self.palette.query()
    }

    pub(crate) fn selected(&self) -> usize {
        self.palette.selected()
    }

    #[cfg(test)]
    pub(crate) fn visible(&self) -> Vec<&PaletteRow<Command>> {
        self.palette.visible()
    }

    /// The rendered rows: each visible row's numbered label (`"N. …"`) paired
    /// with its direct-key shortcut hint, if any.
    pub(crate) fn numbered_entries(&self) -> Vec<(String, Option<&'static str>)> {
        self.palette.numbered_entries()
    }

    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[PaletteRow<Command>] {
        self.palette.rows()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> PaletteStep<Command> {
        self.palette.handle_key(key)
    }
}

fn palette_over(
    title: impl Into<String>,
    subtitle: Option<String>,
    rows: Vec<PaletteRow<Command>>,
) -> CommandPalette<Command> {
    CommandPalette::new(title, subtitle, rows, PaletteControls::COMMANDS)
}
