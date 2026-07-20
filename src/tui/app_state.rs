//! `App` state: construction, query/filter, selection, notes toggles,
//! and view navigation.

use super::*;

use std::{
    collections::HashSet,
    path::PathBuf,
};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use fuzzy_matcher::skim::SkimMatcherV2;
use crate::tasks::cli::Cli;
use crate::config::Config;
use crate::main_view::MainView;
use crate::state::{Db, PanelSide};
use crate::tasks::render::{
    build_body_lines_with_ranges, header_lines,
    no_matches_lines,
};
use crate::tasks::selector::Selector;
use crate::tasks::task::{self, Task};
use crate::tasks::view::{self, View, ViewSpec};

impl<'a> App<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        view: &ViewSpec,
        cli: &'a Cli,
        today: NaiveDate,
        csv_path: PathBuf,
        all_tasks: Vec<Task>,
        all_habits: Vec<Task>,
        active_view: Option<View>,
        initial_search: Option<String>,
        agenda_runner: Box<dyn ShellRunner>,
        habits_runner: Box<dyn ShellRunner>,
        open_runner: Box<dyn ShellRunner>,
        config: Config,
        instance: String,
        brain_root: PathBuf,
        db_path: PathBuf,
        db: Db,
        search: crate::picker::App,
        panel_side: PanelSide,
    ) -> Self {
        let query = initial_search.unwrap_or_default();
        let in_search = !query.is_empty();
        let mut app = Self {
            today,
            // Seeded to the startup date; `run_tui` overwrites it with the
            // current logical day right after the startup triage check so the
            // first same-day refresh doesn't re-fire the nudge.
            triage_day: today,
            config,
            full_notes: cli.display.full_notes,
            expanded_notes: HashSet::new(),
            cli,
            csv_path,
            all_tasks,
            all_habits,
            active_view,
            header: header_lines(view, cli, active_view),
            body_lines: Vec::new(),
            visual_row_offsets: vec![0],
            visible_tasks: Vec::new(),
            task_line_ranges: Vec::new(),
            selected_task: None,
            pending_count: None,
            base_tasks: view.tasks.clone(),
            query,
            in_search,
            matcher: SkimMatcherV2::default().ignore_case(),
            scroll: 0,
            last_inner_height: 1,
            last_content_rows: 1,
            brain: None,
            focus: Panel::Tasks,
            brain_rect: None,
            instance,
            brain_root,
            db_path,
            alert: None,
            pending_brain_submit: 0,
            palette: None,
            brain_input: None,
            confirm: None,
            link_picker: None,
            help: None,
            flash: None,
            agenda_runner,
            habits_runner,
            open_runner,
            db,
            main_view: MainView::Tasks,
            search,
            panel_side,
        };
        app.rebuild_body();
        app
    }

    pub(crate) fn has_active_filter(&self) -> bool {
        !self.query.is_empty()
    }

    pub(crate) fn show_search_bar(&self) -> bool {
        self.in_search || self.has_active_filter()
    }

    pub(crate) const fn max_scroll(&self) -> u16 {
        self.last_content_rows
            .saturating_sub(self.last_inner_height)
    }

    pub(crate) const fn clamp(&mut self) {
        let max = self.max_scroll();
        if self.scroll > max {
            self.scroll = max;
        }
    }

    pub(crate) fn rebuild_body(&mut self) {
        let visible_refs = filter_tasks(&self.base_tasks, &self.query, &self.matcher);
        let visible: Vec<Task> = visible_refs.into_iter().cloned().collect();

        if visible.is_empty() && self.has_active_filter() {
            self.body_lines = no_matches_lines(&self.query);
            self.task_line_ranges.clear();
            self.visible_tasks.clear();
            self.selected_task = None;
            self.scroll = 0;
            return;
        }

        let full = self.full_notes;
        let expanded = &self.expanded_notes;
        let (lines, ranges) =
            build_body_lines_with_ranges(&visible, self.today, |t| {
                full || expanded.contains(&t.id)
            });
        self.body_lines = lines;
        self.task_line_ranges = ranges;
        self.visible_tasks = visible;

        // Clamp / initialize selection: keep the previous index when it
        // still points at a real task (so the user's position survives a
        // search-narrowing); otherwise start at the top.
        self.selected_task = if self.visible_tasks.is_empty() {
            None
        } else {
            let prev = self.selected_task.unwrap_or(0);
            Some(prev.min(self.visible_tasks.len() - 1))
        };

        self.scroll = 0;
        self.ensure_selected_visible();
    }

    pub(crate) fn append_query(&mut self, c: char) {
        self.query.push(c);
        self.rebuild_body();
    }

    pub(crate) fn pop_query(&mut self) {
        if self.query.pop().is_some() {
            self.rebuild_body();
        }
    }

    pub(crate) fn clear_query(&mut self) {
        if !self.query.is_empty() {
            self.query.clear();
            self.rebuild_body();
        }
    }

    /// Approximate how many tasks fit in the current visible area, used for
    /// page-step navigation. Falls back to 1 on tiny terminals.
    pub(crate) fn tasks_per_page(&self) -> usize {
        let h = usize::from(self.last_inner_height.max(1));
        (h / 4).max(1)
    }

    /// Scroll the currently-focused panel a half-page in `up`'s direction.
    /// The brain panel scrolls its vt100 scrollback by half its visible rows;
    /// the tasks panel moves the selection by the same half-page step as bare
    /// `d`/`u`. Bound to Alt+U / Alt+D, which fire even while the brain panel
    /// is focused or the search filter is active.
    pub(crate) fn scroll_focused_half_page(&mut self, up: bool) {
        match self.focus {
            Panel::Brain => {
                if let Some(pty) = self.brain.as_ref() {
                    let step = half_page_step(pty.rows);
                    if up {
                        pty.scroll_up(step);
                    } else {
                        pty.scroll_down(step);
                    }
                }
            }
            Panel::Tasks => {
                let step = (self.tasks_per_page() / 2).max(1);
                if up {
                    self.select_prev(step);
                } else {
                    self.select_next(step);
                }
            }
        }
    }

    pub(crate) fn select_next(&mut self, n: usize) {
        let Some(sel) = self.selected_task else { return };
        let len = self.visible_tasks.len();
        if len == 0 {
            return;
        }
        self.set_selected(sel.saturating_add(n).min(len - 1));
    }

    pub(crate) fn select_prev(&mut self, n: usize) {
        let Some(sel) = self.selected_task else { return };
        self.set_selected(sel.saturating_sub(n));
    }

    pub(crate) fn set_selected(&mut self, idx: usize) {
        if self.visible_tasks.is_empty() {
            self.selected_task = None;
            return;
        }
        let clamped = idx.min(self.visible_tasks.len() - 1);
        if Some(clamped) == self.selected_task {
            return;
        }
        self.selected_task = Some(clamped);
        // Body content is identical regardless of selection — the highlight
        // is a draw-time background block, not a line mutation. So we only
        // need to nudge scroll to keep the new pick visible.
        self.ensure_selected_visible();
    }

    pub(crate) fn select_first(&mut self) {
        self.set_selected(0);
    }

    pub(crate) fn select_last(&mut self) {
        if !self.visible_tasks.is_empty() {
            self.set_selected(self.visible_tasks.len() - 1);
        }
    }

    /// Scroll so the selected task's content is fully on-screen. Called
    /// after every selection change and at the top of `draw_tasks` once
    /// `last_inner_height` is known.
    pub(crate) fn ensure_selected_visible(&mut self) {
        let Some(sel) = self.selected_task else { return };
        let Some(range) = self.task_line_ranges.get(sel) else { return };
        let vis = visual_range(&self.visual_row_offsets, range.clone());
        let start = vis.start;
        let end = vis.end;
        let inner = self.last_inner_height.max(1);

        if start < self.scroll {
            self.scroll = start;
        } else if end > self.scroll.saturating_add(inner) {
            self.scroll = end.saturating_sub(inner);
        }
        self.clamp();
    }

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

    pub(crate) fn current_task_id(&self) -> Option<String> {
        self.selected_task
            .and_then(|i| self.visible_tasks.get(i))
            .map(|t| t.id.clone())
    }

    pub(crate) fn current_is_habit(&self) -> bool {
        self.selected_task
            .and_then(|i| self.visible_tasks.get(i))
            .is_some_and(Task::is_habit)
    }

    /// Whether the selected entry has any non-blank notes. Drives the
    /// "Expand notes" palette command's visibility and whether `l` does
    /// anything.
    pub(crate) fn current_has_notes(&self) -> bool {
        self.selected_task
            .and_then(|i| self.visible_tasks.get(i))
            .is_some_and(|t| !t.notes.trim().is_empty())
    }

    /// The selected entry's link situation (Linear issue and/or notes URLs),
    /// driving the "open link" palette command's visibility and label.
    pub(crate) fn current_link_kind(&self) -> LinkKind {
        let Some(task) = self.selected_task.and_then(|i| self.visible_tasks.get(i)) else {
            return LinkKind::None;
        };
        let links = task_links(task, &self.config.linear_base_url);
        classify_links(task, &links)
    }

    /// Whether the selected entry's notes are currently rendered expanded
    /// (either globally via `full_notes` or via the per-task toggle).
    pub(crate) fn current_notes_expanded(&self) -> bool {
        self.full_notes
            || self
                .current_task_id()
                .is_some_and(|id| self.expanded_notes.contains(&id))
    }

    /// Toggle expanded notes for the selected entry. No-op when nothing is
    /// selected or the entry has no notes (the action isn't offered in that
    /// case, but guard here too so the bare `l` key stays inert).
    pub(crate) fn toggle_notes(&mut self) {
        if !self.current_has_notes() {
            return;
        }
        let Some(id) = self.current_task_id() else {
            return;
        };
        if !self.expanded_notes.remove(&id) {
            self.expanded_notes.insert(id);
        }
        self.rebuild_body();
    }

    /// Expand the selected entry's notes — the `→` arrow alias. No-op
    /// when the entry has no notes or they're already expanded.
    pub(crate) fn expand_notes(&mut self) {
        if self.current_has_notes() && !self.current_notes_expanded() {
            self.toggle_notes();
        }
    }

    /// Collapse the selected entry's notes — the `←` arrow alias. No-op
    /// when the entry has no notes or they're already collapsed.
    pub(crate) fn collapse_notes(&mut self) {
        if self.current_has_notes() && self.current_notes_expanded() {
            self.toggle_notes();
        }
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
