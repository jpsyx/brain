use super::{SkimMatcherV2, filter_tasks};
use crate::tasks::task::test_task;
use crate::users::UserId;

#[test]
fn runtime_assignment_filter_switches_members_and_can_restore_all() {
    let mut pablo = test_task("T1", "not_started");
    pablo.assigned_to = "pablo".to_owned();
    let mut wife = test_task("T2", "not_started");
    wife.assigned_to = "wife".to_owned();
    let tasks = vec![pablo, wife];
    let matcher = SkimMatcherV2::default().ignore_case();
    let pablo_id = UserId::parse("pablo").unwrap();
    let wife_id = UserId::parse("wife").unwrap();

    let pablo_only = filter_tasks(&tasks, "", Some(&pablo_id), &matcher);
    let wife_only = filter_tasks(&tasks, "", Some(&wife_id), &matcher);
    let all = filter_tasks(&tasks, "", None, &matcher);

    assert_eq!(
        pablo_only
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["T1"]
    );
    assert_eq!(
        wife_only
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["T2"]
    );
    assert_eq!(all.len(), 2);
}
