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

use filter::filter_tasks;

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

pub(crate) struct TasksRenderState<'a> {
    pub(crate) header: &'a [Line<'static>],
    pub(crate) assignment_users: &'a [crate::tasks::task::AssignmentUser],
    pub(crate) assignment_filter: Option<&'a UserId>,
    pub(crate) show_search_bar: bool,
    pub(crate) in_search: bool,
    pub(crate) query: &'a str,
    pub(crate) visible_count: usize,
    pub(crate) base_count: usize,
    pub(crate) body_lines: &'a [Line<'static>],
    pub(crate) scroll: u16,
    pub(crate) max_scroll: u16,
    pub(crate) pending_count: Option<usize>,
}

pub(crate) struct TaskRowsSnapshot<'a> {
    pub(crate) tasks: &'a [Task],
    pub(crate) habits: &'a [Task],
}

pub(crate) struct TaskTriageSnapshot<'a> {
    pub(crate) today: NaiveDate,
    pub(crate) habits: &'a [Task],
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

    pub(crate) fn selected_task(&self) -> Option<&Task> {
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

    pub(crate) fn rows_snapshot(&self) -> TaskRowsSnapshot<'_> {
        TaskRowsSnapshot {
            tasks: &self.all_tasks,
            habits: &self.all_habits,
        }
    }

    #[cfg(test)]
    pub(crate) fn query_text(&self) -> &str {
        &self.query
    }

    pub(crate) fn triage_snapshot(&self) -> TaskTriageSnapshot<'_> {
        TaskTriageSnapshot {
            today: self.today,
            habits: &self.all_habits,
        }
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

    pub(crate) fn render_state(&self) -> TasksRenderState<'_> {
        TasksRenderState {
            header: &self.header,
            assignment_users: self.assignment.users(),
            assignment_filter: self.assignment_filter.as_ref(),
            show_search_bar: self.in_search || !self.query.is_empty(),
            in_search: self.in_search,
            query: &self.query,
            visible_count: self.visible_tasks.len(),
            base_count: self.base_tasks.len(),
            body_lines: &self.body_lines,
            scroll: self.scroll,
            max_scroll: self.max_scroll(),
            pending_count: self.pending_count,
        }
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
mod tests {
    use std::collections::BTreeMap;

    use chrono::NaiveDate;
    use clap::Parser;

    use super::{TasksState, TasksStateInit};
    use crate::personalization::tags::TagStyles;
    use crate::tasks::cli::Cli;
    use crate::tasks::selector::Selector;
    use crate::tasks::task::{AssignmentContext, test_task};
    use crate::tasks::view::{TaskViewOptions, View, build_view};
    use crate::users::UserId;

    fn state() -> TasksState {
        let today = NaiveDate::from_ymd_opt(2026, 8, 21).expect("valid date");
        let cli = Cli::parse_from(["tasks"]);
        let options = TaskViewOptions::from(&cli);
        let mut first = test_task("T1", "not_started");
        first.name = "Alpha plan".to_owned();
        first.notes = "A detailed first note\nwith a second line".to_owned();
        first.assigned_to = "alice".to_owned();
        first.due_date = Some(today);
        let mut second = test_task("T2", "not_started");
        second.name = "Beta follow-up".to_owned();
        second.assigned_to = "teammate".to_owned();
        second.due_date = today.succ_opt();
        let all_tasks = vec![first, second];
        let mut habit = test_task("H1", "not_started");
        habit.due_date = Some(today);
        let all_habits = vec![habit];
        let view = build_view(
            &options,
            &Selector::All,
            Some(View::All),
            all_tasks.clone(),
            today,
        );
        let assignment = AssignmentContext::legacy(&crate::actor::test_actor("alice"));

        TasksState::new(TasksStateInit {
            view,
            task_options: options,
            today,
            active_view: Some(View::All),
            all_tasks,
            all_habits,
            assignment,
            assignment_filter: None,
            initial_search: None,
            tag_styles: TagStyles::with_overrides(&BTreeMap::new()),
        })
    }

    #[test]
    fn construction_owns_view_selection_query_and_assignment_filtering() {
        let mut state = state();

        assert_eq!(state.active_view(), Some(View::All));
        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("T1")
        );
        assert_eq!(state.visible_count(), 2);

        state.append_query('b');
        assert_eq!(state.query_text(), "b");
        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("T2")
        );

        state.clear_query();
        state.set_assignment_filter(Some(UserId::parse("alice").expect("valid user")));
        assert_eq!(state.visible_count(), 1);
        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("T1")
        );
    }

    #[test]
    fn notes_body_layout_and_scrolling_remain_one_pure_state_transition() {
        let mut state = state();
        let collapsed_lines = state.render_state().body_lines.len();

        state.toggle_notes();
        assert!(state.current_notes_expanded());
        assert!(state.render_state().body_lines.len() > collapsed_lines);

        state.select_next(1);
        let line_heights = vec![1; state.render_state().body_lines.len()];
        state.update_body_layout(1, &line_heights);

        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("T2")
        );
        assert!(state.scroll_offset() > 0);
        assert!(state.max_scroll() >= state.scroll_offset());
    }

    #[test]
    fn view_navigation_switches_between_task_and_habit_sources() {
        let mut state = state();

        state.set_view(View::Habits);

        assert_eq!(state.active_view(), Some(View::Habits));
        assert_eq!(state.visible_count(), 1);
        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("H1")
        );
        assert_eq!(state.assignment_snapshot().actor_id.as_str(), "alice");
    }

    #[test]
    fn advancing_the_day_rematerializes_date_relative_rows_without_io() {
        let mut state = state();
        state.set_view(View::Today);
        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("T1")
        );

        state.advance_day(NaiveDate::from_ymd_opt(2026, 8, 22).expect("valid date"));

        assert_eq!(state.render_state().visible_count, 2);
        assert!(
            state
                .render_state()
                .body_lines
                .iter()
                .any(|line| line.to_string().contains("Beta follow-up"))
        );
    }
}
