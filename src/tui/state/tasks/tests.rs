use std::collections::BTreeMap;

use chrono::NaiveDate;
use clap::Parser;

use super::{TaskLinksPlan, TasksState, TasksStateInit};
use crate::personalization::tags::TagStyles;
use crate::tasks::cli::Cli;
use crate::tasks::selector::Selector;
use crate::tasks::task::{AssignmentContext, test_task};
use crate::tasks::view::{TaskViewOptions, View, build_view};
use crate::tui::links::LinkKind;
use crate::users::UserId;

fn state() -> TasksState {
    let today = NaiveDate::from_ymd_opt(2026, 8, 21).expect("valid date");
    let cli = Cli::parse_from(["tasks"]);
    let options = TaskViewOptions::from(&cli);
    let mut first = test_task("T1", "not_started");
    first.name = "Alpha plan".to_owned();
    first.notes = "A detailed first note\nwith a second line".to_owned();
    first.see_also = "https://reference.example/alpha".to_owned();
    first.linear_issue = "OPS-17".to_owned();
    first.assigned_to = "alice".to_owned();
    first.due_date = Some(today);
    let mut second = test_task("T2", "not_started");
    second.name = "Beta follow-up".to_owned();
    second.assigned_to = "teammate".to_owned();
    second.due_date = today.succ_opt();
    let all_tasks = vec![first, second];
    let mut habit = test_task("H1", "not_started");
    habit.name = "Morning Triage".to_owned();
    habit.system_key = crate::tasks::triage_habits::DAILY_SYSTEM_KEY.to_owned();
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
    let collapsed_lines = state.panel_model().content().len();

    state.toggle_notes();
    assert!(state.current_notes_expanded());
    assert!(state.panel_model().content().len() > collapsed_lines);

    state.select_next(1);
    let line_heights = vec![1; state.panel_model().content().len()];
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

    assert_eq!(state.panel_model().visible_count(), 2);
    assert!(
        state
            .panel_model()
            .content()
            .any(|line| line.to_string().contains("Beta follow-up"))
    );
}

#[test]
fn selected_link_planning_keeps_task_policy_inside_the_owner() {
    let state = state();

    assert_eq!(
        state.selected_link_kind("https://linear.example/issue/"),
        LinkKind::Multiple
    );
    let TaskLinksPlan::Choose { task_id, links } =
        state.selected_links_plan("https://linear.example/issue/")
    else {
        panic!("the selected task has two destinations");
    };
    assert_eq!(task_id, "T1");
    assert_eq!(
        links
            .iter()
            .map(|link| link.url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://linear.example/issue/OPS-17",
            "https://reference.example/alpha"
        ]
    );
}

#[test]
fn removal_validation_searches_owned_tasks_and_habits_without_row_escape() {
    let state = state();
    let config = crate::config::Config {
        enable_triage_habits: true,
        ..crate::config::Config::default()
    };

    assert!(state.validate_removal("H1", &config).is_err());
    assert!(state.validate_removal("missing", &config).is_ok());
}

#[test]
fn daily_triage_planning_returns_only_the_modal_target() {
    let state = state();

    let target = state
        .daily_triage_nudge(true, false, "morning triage")
        .expect("outstanding triage target");
    assert_eq!(target.task_id, "H1");
    assert_eq!(target.task_label, "Morning Triage");
    assert_eq!(
        state.daily_triage_date(),
        NaiveDate::from_ymd_opt(2026, 8, 21).expect("valid date")
    );
    assert!(
        state
            .daily_triage_nudge(true, true, "morning triage")
            .is_none()
    );
}

#[test]
fn panel_model_streams_render_content_without_exposing_the_owned_collection() {
    let state = state();
    let panel = state.panel_model();

    let content = panel.content().map(ToString::to_string).collect::<Vec<_>>();
    assert!(content.iter().any(|line| line.contains("Alpha plan")));
    assert_eq!(panel.visible_count(), 2);
}
