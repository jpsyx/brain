//! Tab-cycle view switching and CSV reloads: choosing the data source for a
//! view, rebuilding the header + base task list on a switch, reloading both
//! CSVs after a mutating action, and stepping through the view cycle.

use anyhow::{Context, Result};

use crate::tasks::render::header_lines;
use crate::tasks::selector::Selector;
use crate::tasks::task;
use crate::tasks::view;
use crate::tui::*;

impl App<'_> {
    /// Data source for a given view: habits.csv for `View::Habits`,
    /// tasks.csv for everything else (including the `None` "custom
    /// selector" case).
    pub(crate) fn data_for_view(&self, view: Option<View>) -> Vec<Task> {
        if view == Some(View::Habits) {
            self.all_habits.clone()
        } else {
            self.all_tasks.clone()
        }
    }

    /// Switch to a new view, rebuild the underlying task list from the
    /// matching data source, reset search/scroll/selection, and refresh
    /// the header.
    pub(crate) fn set_view(&mut self, view: View) {
        self.active_view = Some(view);
        self.query.clear();
        self.in_search = false;
        let spec = view::build_view(
            self.cli,
            &view.selector(self.today),
            Some(view),
            self.data_for_view(Some(view)),
            self.today,
        );
        self.header = header_lines(&spec, self.cli, Some(view));
        self.base_tasks = spec.tasks;
        // Switching views is a fresh context — start selection at the top
        // rather than carrying the previous index across an unrelated list.
        self.selected_task = Some(0);
        self.rebuild_body();
    }

    /// Re-read tasks.csv + habits.csv and rebuild the view in-place. Called
    /// after palette actions that mutate either CSV (mark-complete on a
    /// task or a habit).
    pub(crate) fn reload_tasks(&mut self) -> Result<()> {
        self.all_tasks = task::load_tasks(&self.csv_path)
            .with_context(|| format!("reading {}", self.csv_path.display()))?;
        let habits_path = self.csv_path.with_file_name("habits.csv");
        self.all_habits = task::load_habits(&habits_path).unwrap_or_default();
        let selector = self
            .active_view
            .map_or(Selector::All, |v| v.selector(self.today));
        let spec = view::build_view(
            self.cli,
            &selector,
            self.active_view,
            self.data_for_view(self.active_view),
            self.today,
        );
        self.header = header_lines(&spec, self.cli, self.active_view);
        self.base_tasks = spec.tasks;
        // Keep the current `selected_task` index — when a completed task
        // disappears, the same index now points at what was the next task.
        self.rebuild_body();
        Ok(())
    }

    pub(crate) fn cycle_view_next(&mut self) {
        let next = self.active_view.map_or(View::Today, View::next);
        self.set_view(next);
    }

    pub(crate) fn cycle_view_prev(&mut self) {
        // From a custom view (`active_view == None`), Shift+Tab lands on
        // the last view in the cycle so it mirrors Tab landing on `Today`
        // from the front.
        let prev = self.active_view.map_or_else(
            || *View::CYCLE.last().expect("CYCLE is non-empty"),
            View::prev,
        );
        self.set_view(prev);
    }
}
