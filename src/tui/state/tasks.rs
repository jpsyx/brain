use std::collections::HashSet;
use std::ops::Range;

use chrono::NaiveDate;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::text::Line;

use crate::personalization::tags::TagStyles;
use crate::tasks::render::{build_body_lines_with_ranges, header_lines, no_matches_lines};
use crate::tasks::selector::Selector;
use crate::tasks::task::{AssignmentContext, AssignmentUiMode, AssignmentUser, Task};
use crate::tasks::view::{self, TaskViewOptions, View, ViewSpec};
use crate::users::UserId;

mod filter;
mod interaction;
mod links;
mod panel;
mod policy;
mod triage;

use filter::filter_tasks;
pub(crate) use links::TaskLinksPlan;

pub(crate) struct TasksStateInit {
    pub(crate) view: ViewSpec,
    pub(crate) task_options: TaskViewOptions,
    pub(crate) today: NaiveDate,
    pub(crate) active_view: Option<View>,
    pub(crate) all_tasks: Vec<Task>,
    pub(crate) all_habits: Vec<Task>,
    pub(crate) assignment: AssignmentContext,
    pub(crate) assignment_filter: Option<UserId>,
    pub(crate) initial_search: Option<String>,
    pub(crate) tag_styles: TagStyles,
}

pub(crate) struct TasksState {
    tag_styles: TagStyles,
    today: NaiveDate,
    full_notes: bool,
    expanded_notes: HashSet<String>,
    task_options: TaskViewOptions,
    all_tasks: Vec<Task>,
    all_habits: Vec<Task>,
    active_view: Option<View>,
    base_tasks: Vec<Task>,
    header: Vec<Line<'static>>,
    query: String,
    in_search: bool,
    matcher: SkimMatcherV2,
    assignment: AssignmentContext,
    assignment_filter: Option<UserId>,
    visible_tasks: Vec<Task>,
    task_line_ranges: Vec<Range<usize>>,
    selected_task: Option<usize>,
    pending_count: Option<usize>,
    body_lines: Vec<Line<'static>>,
    visual_row_offsets: Vec<u16>,
    scroll: u16,
    last_inner_height: u16,
    last_content_rows: u16,
}

pub(crate) struct TaskAssignmentSnapshot<'a> {
    pub(crate) mode: AssignmentUiMode,
    pub(crate) actor_id: &'a UserId,
    pub(crate) users: &'a [AssignmentUser],
    pub(crate) filter: Option<&'a UserId>,
}

impl TasksState {
    pub(crate) fn new(init: TasksStateInit) -> Self {
        let TasksStateInit {
            view,
            task_options,
            today,
            active_view,
            all_tasks,
            all_habits,
            assignment,
            assignment_filter,
            initial_search,
            tag_styles,
        } = init;
        let query = initial_search.unwrap_or_default();
        let in_search = !query.is_empty();
        let header = header_lines(&view, &task_options, active_view);
        let mut state = Self {
            tag_styles,
            today,
            full_notes: task_options.full_notes,
            expanded_notes: HashSet::new(),
            task_options,
            all_tasks,
            all_habits,
            active_view,
            base_tasks: view.tasks,
            header,
            query,
            in_search,
            matcher: SkimMatcherV2::default().ignore_case(),
            assignment,
            assignment_filter,
            visible_tasks: Vec::new(),
            task_line_ranges: Vec::new(),
            selected_task: None,
            pending_count: None,
            body_lines: Vec::new(),
            visual_row_offsets: vec![0],
            scroll: 0,
            last_inner_height: 1,
            last_content_rows: 1,
        };
        state.rebuild_body();
        state
    }

    #[cfg(test)]
    pub(crate) const fn active_view(&self) -> Option<View> {
        self.active_view
    }

    pub(crate) fn advance_day(&mut self, today: NaiveDate) {
        self.today = today;
        self.rematerialize_active_view();
    }

    fn selected_task(&self) -> Option<&Task> {
        self.selected_task
            .and_then(|index| self.visible_tasks.get(index))
    }

    pub(crate) fn selected_identity(&self) -> Option<(String, String)> {
        self.selected_task()
            .map(|task| (task.id.clone(), task.name.clone()))
    }

    #[cfg(test)]
    pub(crate) const fn visible_count(&self) -> usize {
        self.visible_tasks.len()
    }

    #[cfg(test)]
    pub(crate) fn query_text(&self) -> &str {
        &self.query
    }

    pub(crate) fn assignment_snapshot(&self) -> TaskAssignmentSnapshot<'_> {
        TaskAssignmentSnapshot {
            mode: self.assignment.mode(),
            actor_id: self.assignment.actor_id(),
            users: self.assignment.users(),
            filter: self.assignment_filter.as_ref(),
        }
    }

    pub(crate) const fn is_searching(&self) -> bool {
        self.in_search
    }

    pub(crate) fn enter_search(&mut self) {
        self.in_search = true;
    }

    pub(crate) fn leave_search(&mut self) {
        self.in_search = false;
    }

    pub(crate) fn query_is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub(crate) fn has_active_filter(&self) -> bool {
        !self.query.is_empty() || self.assignment_filter.is_some()
    }

    pub(crate) fn set_view(&mut self, active_view: View) {
        self.active_view = Some(active_view);
        self.query.clear();
        self.in_search = false;
        let spec = view::build_view(
            &self.task_options,
            &active_view.selector(self.today),
            Some(active_view),
            self.data_for_view(Some(active_view)),
            self.today,
        );
        self.header = header_lines(&spec, &self.task_options, Some(active_view));
        self.base_tasks = spec.tasks;
        self.selected_task = Some(0);
        self.rebuild_body();
    }

    pub(crate) fn cycle_view_next(&mut self) {
        self.set_view(self.active_view.map_or(View::Today, View::next));
    }

    pub(crate) fn cycle_view_prev(&mut self) {
        let previous = self.active_view.map_or_else(
            || *View::CYCLE.last().expect("CYCLE is non-empty"),
            View::prev,
        );
        self.set_view(previous);
    }

    pub(crate) fn replace_rows(&mut self, all_tasks: Vec<Task>, all_habits: Vec<Task>) {
        self.all_tasks = all_tasks;
        self.all_habits = all_habits;
        self.rematerialize_active_view();
    }

    #[cfg(test)]
    pub(crate) fn contains_task_named(&self, name: &str) -> bool {
        self.all_tasks.iter().any(|task| task.name == name)
    }

    fn rematerialize_active_view(&mut self) {
        let selector = self.active_view.map_or(Selector::All, |active_view| {
            active_view.selector(self.today)
        });
        let spec = view::build_view(
            &self.task_options,
            &selector,
            self.active_view,
            self.data_for_view(self.active_view),
            self.today,
        );
        self.header = header_lines(&spec, &self.task_options, self.active_view);
        self.base_tasks = spec.tasks;
        self.rebuild_body();
    }

    fn data_for_view(&self, active_view: Option<View>) -> Vec<Task> {
        if active_view == Some(View::Habits) {
            self.all_habits.clone()
        } else {
            self.all_tasks.clone()
        }
    }

    fn rebuild_body(&mut self) {
        let visible = filter_tasks(
            &self.base_tasks,
            &self.query,
            self.assignment_filter.as_ref(),
            &self.matcher,
        )
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();

        if visible.is_empty() && self.has_active_filter() {
            let description = self.assignment_filter.as_ref().map_or_else(
                || self.query.clone(),
                |user_id| {
                    if self.query.is_empty() {
                        format!("assigned to {user_id}")
                    } else {
                        format!("{} assigned to {user_id}", self.query)
                    }
                },
            );
            self.body_lines = no_matches_lines(&description);
            self.task_line_ranges.clear();
            self.visible_tasks.clear();
            self.selected_task = None;
            self.scroll = 0;
            return;
        }

        let full_notes = self.full_notes;
        let expanded_notes = &self.expanded_notes;
        let (lines, ranges) = build_body_lines_with_ranges(
            &visible,
            self.today,
            self.assignment.mode().show_in_detail,
            &self.tag_styles,
            |task| full_notes || expanded_notes.contains(&task.id),
        );
        self.body_lines = lines;
        self.task_line_ranges = ranges;
        self.visible_tasks = visible;
        self.selected_task = if self.visible_tasks.is_empty() {
            None
        } else {
            Some(
                self.selected_task
                    .unwrap_or(0)
                    .min(self.visible_tasks.len() - 1),
            )
        };
        self.scroll = 0;
        self.ensure_selected_visible();
    }
}

#[cfg(test)]
mod tests;
