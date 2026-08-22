//! Reload the task and habit stores into the focused task-list aggregate.

use anyhow::{Context, Result};

use crate::tasks::task;
use crate::tui::*;

impl App {
    /// Re-read tasks.csv + habits.csv and rebuild the view in-place. Called
    /// after palette actions that mutate either CSV (mark-complete on a
    /// task or a habit).
    pub(crate) fn reload_tasks(&mut self) -> Result<()> {
        let all_tasks = task::load_tasks(self.context.tasks_csv_path())
            .with_context(|| format!("reading {}", self.context.tasks_csv_path().display()))?;
        let habits_path = self.context.tasks_csv_path().with_file_name("habits.csv");
        let all_habits = task::load_habits(&habits_path).unwrap_or_default();
        self.tasks.replace_rows(all_tasks, all_habits);
        Ok(())
    }
}
