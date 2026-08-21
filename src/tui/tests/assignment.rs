use crate::tasks::task::{AssignmentUiMode, AssignmentUser};
use crate::tasks::view::ViewSpec;
use crate::tui::{
    AssigneeFilterState, EscapeAction, assignee_filter_line, normal_escape_action,
    tasks_header_height, tasks_header_lines,
};
use crate::users::UserId;
use clap::Parser;

fn user(id: &str, name: &str) -> AssignmentUser {
    AssignmentUser {
        id: UserId::parse(id).expect("valid user id"),
        name: name.to_owned(),
    }
}

fn startup_header() -> Vec<ratatui::text::Line<'static>> {
    let cli =
        crate::tasks::cli::Cli::parse_from(["tasks", "--assigned-to", "wife", "--priority", "p1"]);
    let view = ViewSpec {
        title: "All tasks".to_owned(),
        subtitle: String::new(),
        tasks: Vec::new(),
        total: 0,
    };
    crate::tasks::render::header_lines(
        &view,
        &crate::tasks::view::TaskViewOptions::from(&cli),
        None,
    )
}

fn header_text(filter: Option<&UserId>) -> String {
    tasks_header_lines(
        &startup_header(),
        &[user("pablo", "Pablo"), user("wife", "Wife")],
        filter,
    )
    .iter()
    .flat_map(|line| line.spans.iter())
    .map(|span| span.content.as_ref())
    .collect()
}

#[test]
fn assignee_filter_picker_exposes_all_members_and_the_clear_choice() {
    let wife = UserId::parse("wife").expect("valid user id");
    let mut picker =
        AssigneeFilterState::new(&[user("pablo", "Pablo"), user("wife", "Wife")], Some(&wife));

    assert_eq!(
        picker.rows(),
        ["All assignees", "Pablo (pablo)", "Wife (wife)"]
    );
    assert_eq!(picker.selected(), 2);
    assert_eq!(picker.selected_user(), Some(wife));

    picker.move_up();
    picker.move_up();
    assert_eq!(picker.selected(), 0);
    assert_eq!(picker.selected_user(), None);
    picker.move_down();
    assert_eq!(
        picker.selected_user(),
        Some(UserId::parse("pablo").expect("valid user id"))
    );
}

#[test]
fn assignee_filter_picker_number_selection_is_one_based_and_bounded() {
    let mut picker = AssigneeFilterState::new(&[user("pablo", "Pablo")], None);

    assert!(picker.select_number(2));
    assert_eq!(
        picker.selected_user(),
        Some(UserId::parse("pablo").expect("valid user id"))
    );
    assert!(!picker.select_number(3));
    assert_eq!(picker.selected(), 1);
}

#[test]
fn active_assignee_filter_line_names_the_portable_member() {
    let line = assignee_filter_line("Wife", "wife");
    let text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert!(text.contains("ASSIGNEE"));
    assert!(text.contains("Wife"));
    assert!(text.contains("wife"));
}

#[test]
fn assignment_banner_receives_its_own_header_row() {
    assert_eq!(tasks_header_height(4), 4);
}

#[test]
fn escape_clears_a_startup_assignment_filter_before_quitting() {
    let one_user_mode = AssignmentUiMode {
        show_in_detail: false,
        show_create_control: false,
        show_reassign_control: false,
        show_filter: false,
    };

    assert!(!one_user_mode.show_filter);
    assert_eq!(normal_escape_action(true), EscapeAction::ClearFilters);
    assert_eq!(normal_escape_action(false), EscapeAction::Quit);
}

#[test]
fn startup_header_shows_only_the_live_assignment_state() {
    let wife = UserId::parse("wife").expect("valid user id");

    let text = header_text(Some(&wife));

    assert!(text.contains("priority=p1"));
    assert!(text.contains("Wife (wife)"));
    assert!(!text.contains("assigned_to=wife"));
}

#[test]
fn switched_header_replaces_the_startup_assignment_state() {
    let pablo = UserId::parse("pablo").expect("valid user id");

    let text = header_text(Some(&pablo));

    assert!(text.contains("Pablo (pablo)"));
    assert!(!text.contains("wife"));
}

#[test]
fn all_assignees_header_removes_the_startup_assignment_state() {
    let text = header_text(None);

    assert!(text.contains("priority=p1"));
    assert!(!text.contains("wife"));
    assert!(!text.contains("ASSIGNEE"));
}

#[test]
fn escape_cleared_header_removes_the_startup_assignment_state() {
    let text = header_text(None);

    assert!(!text.contains("wife"));
    assert!(!text.contains("ASSIGNEE"));
}
