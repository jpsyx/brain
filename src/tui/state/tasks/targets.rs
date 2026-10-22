//! Addressing a task by ID rather than by highlight.
//!
//! The command palette can run a task command from any view, so every
//! task-scoped operation needs a by-ID entry point alongside its
//! act-on-the-highlight one.

use super::TasksState;
use crate::tasks::task::Task;
use crate::tui::palette::{TaskChoice, TaskContext};

impl TasksState {
    /// The highlighted entry as palette context, or `None` when nothing is
    /// highlighted.
    pub(crate) fn selected_task_context(&self, linear_base: &str) -> Option<TaskContext> {
        let (id, label) = self.selected_identity()?;
        Some(TaskContext {
            id,
            label,
            is_habit: self.current_is_habit(),
            has_notes: self.current_has_notes(),
            notes_expanded: self.current_notes_expanded(),
            links: self.selected_link_kind(linear_base),
        })
    }

    /// Every row a task command could be pointed at, tasks first and habits
    /// after, drawn from the whole store rather than the active sub-view — the
    /// user is choosing a target, not navigating a list.
    pub(crate) fn target_choices(&self, include_habits: bool) -> Vec<TaskChoice> {
        let habits = if include_habits {
            self.all_habits.as_slice()
        } else {
            &[]
        };
        self.all_tasks
            .iter()
            .chain(habits)
            .map(|task| TaskChoice {
                id: task.id.clone(),
                label: task.name.clone(),
                is_habit: task.is_habit(),
            })
            .collect()
    }

    /// Flip one entry's notes by ID. Returns whether there were notes to flip;
    /// expansion is keyed by ID, so this works for an entry the active
    /// sub-view isn't currently showing.
    pub(crate) fn toggle_notes_for(&mut self, id: &str) -> bool {
        if self
            .row_with_id(id)
            .is_none_or(|task| task.notes.trim().is_empty())
        {
            return false;
        }
        if !self.expanded_notes.remove(id) {
            self.expanded_notes.insert(id.to_owned());
        }
        self.rebuild_body();
        true
    }

    /// One entry from the whole store, by ID.
    pub(super) fn row_with_id(&self, id: &str) -> Option<&Task> {
        self.all_tasks
            .iter()
            .chain(self.all_habits.iter())
            .find(|task| task.id == id)
    }
}
