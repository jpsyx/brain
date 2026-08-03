use crate::tasks::task::{AssignmentUiMode, AssignmentUser};
use crate::tui::{
    AssigneeFilterState, EscapeAction, assignee_filter_line, normal_escape_action,
    tasks_header_height,
};
use crate::users::UserId;

fn user(id: &str, name: &str) -> AssignmentUser {
    AssignmentUser {
        id: UserId::parse(id).expect("valid user id"),
        name: name.to_owned(),
    }
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
