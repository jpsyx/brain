//! The two "which one?" pickers a command raises when the palette row it came
//! from had no target in context.
//!
//! This is the other half of the parent-set invariant: because every command is
//! always listed, running one from the wrong view must still be able to reach a
//! target. A task command asks with a filterable task list; an entry command
//! asks with the same fuzzy brain-directory picker the search view uses.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::command::{EntryCommand, TaskCommand};
use super::model::{CommandPalette, PaletteControls, PaletteRow, PaletteStep};

/// One row of the task picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskChoice {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) is_habit: bool,
}

/// A task command waiting for the user to say which task.
pub(crate) struct TaskTargetPicker {
    command: TaskCommand,
    choices: Vec<TaskChoice>,
    palette: CommandPalette<usize>,
}

impl TaskTargetPicker {
    /// Build the picker for `command` over `choices`, which the caller has
    /// already narrowed to rows the command accepts (habits are excluded for a
    /// tasks-only command).
    pub(crate) fn new(command: TaskCommand, choices: Vec<TaskChoice>) -> Self {
        let rows = choices
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                PaletteRow::new(format!("{} · {}", choice.id, choice.label), index, None)
            })
            .collect();
        Self {
            command,
            choices,
            palette: CommandPalette::new(
                command.picker_title(),
                None,
                rows,
                PaletteControls::COMMANDS,
            ),
        }
    }

    pub(crate) const fn command(&self) -> TaskCommand {
        self.command
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.choices.is_empty()
    }

    pub(crate) fn title(&self) -> &str {
        self.palette.title()
    }

    pub(crate) fn query(&self) -> &str {
        self.palette.query()
    }

    pub(crate) fn selected(&self) -> usize {
        self.palette.selected()
    }

    pub(crate) fn numbered_entries(&self) -> Vec<(String, Option<&'static str>)> {
        self.palette.numbered_entries()
    }

    /// Route a keystroke; a confirmation resolves to the chosen task.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> PaletteStep<TaskChoice> {
        match self.palette.handle_key(key) {
            PaletteStep::Continue => PaletteStep::Continue,
            PaletteStep::Cancel => PaletteStep::Cancel,
            PaletteStep::Confirm(index) => self
                .choices
                .get(index)
                .cloned()
                .map_or(PaletteStep::Continue, PaletteStep::Confirm),
        }
    }
}

/// An entry command waiting for the user to say which file or directory.
pub(crate) struct EntryTargetPicker {
    command: EntryCommand,
    picker: crate::picker::App,
}

/// What a keystroke did to the entry picker.
pub(crate) enum EntryPickerStep {
    Continue,
    Cancel,
    Confirm(std::path::PathBuf),
}

impl EntryTargetPicker {
    pub(crate) const fn new(command: EntryCommand, picker: crate::picker::App) -> Self {
        Self { command, picker }
    }

    pub(crate) const fn command(&self) -> EntryCommand {
        self.command
    }

    pub(crate) const fn title(&self) -> &'static str {
        self.command.picker_title()
    }

    pub(crate) const fn picker_mut(&mut self) -> &mut crate::picker::App {
        &mut self.picker
    }

    /// Route a keystroke. The bindings mirror the brain-directory view so the
    /// picker feels the same wherever it is raised.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> EntryPickerStep {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return EntryPickerStep::Cancel,
            KeyCode::Char('c') if ctrl => return EntryPickerStep::Cancel,
            KeyCode::Enter => {
                return self
                    .picker
                    .selected_path()
                    .map_or(EntryPickerStep::Continue, EntryPickerStep::Confirm);
            }
            KeyCode::Up => self.picker.move_up(),
            KeyCode::Down => self.picker.move_down(),
            KeyCode::Char('k' | 'K' | 'p' | 'P') if ctrl => self.picker.move_up(),
            KeyCode::Char('j' | 'J' | 'n' | 'N') if ctrl => self.picker.move_down(),
            KeyCode::PageUp => self.picker.page_up(),
            KeyCode::PageDown => self.picker.page_down(),
            KeyCode::Home => self.picker.jump_first(),
            KeyCode::End => self.picker.jump_last(),
            KeyCode::Backspace => self.picker.pop_query(),
            KeyCode::Char('u' | 'U') if ctrl => self.picker.clear_query(),
            KeyCode::Char('w' | 'W') if ctrl => self.picker.delete_word(),
            KeyCode::Char(character) if !ctrl && !alt => self.picker.push_query(character),
            _ => {}
        }
        EntryPickerStep::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices() -> Vec<TaskChoice> {
        vec![
            TaskChoice {
                id: "T1".into(),
                label: "Draft the plan".into(),
                is_habit: false,
            },
            TaskChoice {
                id: "T2".into(),
                label: "Ship the thing".into(),
                is_habit: false,
            },
        ]
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn the_title_names_the_command_the_user_is_choosing_a_task_for() {
        let picker = TaskTargetPicker::new(TaskCommand::MarkComplete, choices());
        assert_eq!(picker.title(), "Mark which entry complete?");
    }

    #[test]
    fn rows_carry_the_id_and_the_name_so_either_one_filters() {
        let mut picker = TaskTargetPicker::new(TaskCommand::Start, choices());
        for character in "ship".chars() {
            picker.handle_key(key(KeyCode::Char(character)));
        }
        let rows = picker.numbered_entries();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].0.contains("T2"), "{rows:?}");
    }

    #[test]
    fn confirming_resolves_to_the_highlighted_task() {
        let mut picker = TaskTargetPicker::new(TaskCommand::Remove, choices());
        picker.handle_key(key(KeyCode::Down));
        match picker.handle_key(key(KeyCode::Enter)) {
            PaletteStep::Confirm(choice) => assert_eq!(choice.id, "T2"),
            _ => panic!("Enter should confirm the highlighted row"),
        }
    }

    #[test]
    fn an_empty_picker_confirms_nothing_and_reports_itself_empty() {
        let mut picker = TaskTargetPicker::new(TaskCommand::Defer(1), Vec::new());
        assert!(picker.is_empty());
        assert!(matches!(
            picker.handle_key(key(KeyCode::Enter)),
            PaletteStep::Continue
        ));
    }

    #[test]
    fn escape_cancels_the_task_picker() {
        let mut picker = TaskTargetPicker::new(TaskCommand::Reassign, choices());
        assert!(matches!(
            picker.handle_key(key(KeyCode::Esc)),
            PaletteStep::Cancel
        ));
    }

    #[test]
    fn escape_cancels_the_entry_picker_and_typing_filters_it() {
        let mut picker =
            EntryTargetPicker::new(EntryCommand::Delete, crate::picker::App::new(&[], ""));
        assert!(matches!(
            picker.handle_key(key(KeyCode::Char('a'))),
            EntryPickerStep::Continue
        ));
        assert!(matches!(
            picker.handle_key(key(KeyCode::Esc)),
            EntryPickerStep::Cancel
        ));
    }
}
