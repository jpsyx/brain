use super::moved_summary;
use crate::users::UserId;

#[test]
fn the_move_summary_counts_tasks_in_plain_singular_and_plural_english() {
    let to = UserId::parse("pablo").unwrap();

    assert_eq!(
        moved_summary(0, "ghost", &to),
        "No task or habit is assigned to ghost"
    );
    assert_eq!(moved_summary(1, "me", &to), "Moved 1 task from me to pablo");
    assert_eq!(
        moved_summary(4, "me", &to),
        "Moved 4 tasks from me to pablo"
    );
}

#[test]
fn adding_a_user_prompts_until_both_email_directions_are_supplied() {
    let action = crate::cli::UserAction::Add {
        id: Some("alex".into()),
        name: Some("Alex".into()),
        phone: vec![],
        email: vec!["alex@example.com".into()],
        response_email: None,
    };
    assert!(super::needs_prompt(&action));
}
