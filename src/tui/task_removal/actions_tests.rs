use super::{Action, choices};
use crate::{
    tasks::removal::model::{Dependents, ExternalAction, LinkedIssue, Problem},
    tui::task_removal::{RemovalScreen, render::tests::preview_state},
};

#[test]
fn only_related_tasks_and_issues_offer_extra_choices() {
    let mut state = preview_state();
    let RemovalScreen::Preview(plan) = &mut state.screen else {
        unreachable!()
    };
    plan.edits.clear();
    assert_eq!(
        choices(&state).iter().map(|r| r.0).collect::<Vec<_>>(),
        [Action::Close, Action::Delete]
    );
    let RemovalScreen::Preview(plan) = &mut state.screen else {
        unreachable!()
    };
    plan.external.push(ExternalAction {
        adapter: "tracker".into(),
        issue: "DEMO-1".into(),
        action_id: "private".into(),
    });
    assert_eq!(
        choices(&state).iter().map(|r| r.0).collect::<Vec<_>>(),
        [
            Action::Close,
            Action::Delete,
            Action::Issue(LinkedIssue::Keep),
            Action::Issue(LinkedIssue::Cancel)
        ]
    );
}

#[test]
fn blocked_deletion_offers_resolution_but_never_delete() {
    let mut state = preview_state();
    let RemovalScreen::Preview(plan) = &mut state.screen else {
        unreachable!()
    };
    plan.blockers.push(Problem::new(
        "dependents_exist",
        "T2 needs T1",
        serde_json::json!({}),
    ));
    assert_eq!(
        choices(&state).iter().map(|r| r.0).collect::<Vec<_>>(),
        [
            Action::Close,
            Action::Dependencies(Dependents::Rewire),
            Action::Dependencies(Dependents::Unlink)
        ]
    );
    state.operation_id = Some("saved".into());
    assert_eq!(
        choices(&state).iter().map(|r| r.0).collect::<Vec<_>>(),
        [
            Action::Close,
            Action::Status,
            Action::Resume,
            Action::Abandon
        ]
    );
    state.screen = RemovalScreen::Running {
        operation_id: state.operation_id.clone(),
    };
    assert_eq!(
        choices(&state).iter().map(|r| r.0).collect::<Vec<_>>(),
        [Action::Close]
    );
}

#[test]
fn kept_linked_issue_remains_an_explicit_choice_without_a_cancellation_effect() {
    use crate::tasks::{
        identity::{CsvKind, TaskUuid},
        removal::model::{FileEdit, Identity},
    };
    let mut state = preview_state();
    let RemovalScreen::Preview(plan) = &mut state.screen else {
        unreachable!()
    };
    plan.targets.push(Identity {
        csv: CsvKind::Tasks,
        task_id: "T1".into(),
        task_name: "Example task".into(),
        task_uuid: TaskUuid::new(),
    });
    plan.edits = vec![FileEdit {
        relative: "tasks/tasks.csv".into(),
        before: Some(b"task_id,linear_issue\nT1,DEMO-1\n".to_vec()),
        after: b"task_id,linear_issue\n".to_vec(),
    }];
    let rows = choices(&state);
    assert!(
        rows.iter()
            .any(|row| row.0 == Action::Issue(LinkedIssue::Cancel))
    );
    assert!(
        rows.iter()
            .any(|row| row.0 == Action::Issue(LinkedIssue::Keep) && row.1.starts_with('\u{25cf}'))
    );
}
