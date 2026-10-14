use super::{RemovalScreen, RemovalState, lines};
use crate::tasks::removal::model::{FileEdit, Options, RemovalPlan};
#[test]
fn native_task_removal_preview_shows_relationship_effects_and_plain_policy_choices() {
    let state = RemovalState {
        generation: 1,
        targets: vec!["T1".into()],
        options: Options::default(),
        operation_id: None,
        notice: String::new(),
        abandon_confirm: false,
        scroll: 0,
        screen: RemovalScreen::Preview(RemovalPlan {
            maintenance: None,
            fingerprint: "consent".into(),
            targets: vec![],
            options: Options::default(),
            edits: vec![FileEdit {
                relative: "tasks/tasks.csv".into(),
                before: Some(b"task_id,blocked_by,see_also\nT2,T1,T1|T9\n".to_vec()),
                after: b"task_id,blocked_by,see_also\nT2,T3,T9\n".to_vec(),
            }],
            external: vec![],
            successors: std::collections::BTreeMap::new(),
            blockers: vec![],
            warnings: vec![],
        }),
    };
    let text = lines(&state).join("\n");
    assert!(text.contains("Dependencies T2: T1 -> T3"), "{text}");
    assert!(text.contains("References T2: T1|T9 -> T9"), "{text}");
    assert!(text.contains("Keep prerequisites (rewire)"));
    assert!(text.contains("Remove dependency links (unlink)"));
}
