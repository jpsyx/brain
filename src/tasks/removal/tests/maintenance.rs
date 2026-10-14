use super::fixture::{WorkspaceFixture, needles};
use crate::tasks::complete::read_csv;
use crate::tasks::removal::{
    apply,
    maintenance::{MaintenanceKind, preview_maintenance},
    model::Options,
    preview, resume,
};
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}
fn purge_fixture() -> WorkspaceFixture {
    let f = WorkspaceFixture::new();
    f.write("tasks/tasks.csv", b"task_id,task_name,status,backlogged_date,project,blocked_by,last_touched\nT1,Old,backlog,2000-01-01,atlas,,\nT2,Older,backlog,2000-02-01,atlas,T1,\nT3,Review,not_started,,atlas,T2,\n");
    f.project("projects/atlas");
    f.write("projects/atlas/notes.md", b"Original notes\n");
    f
}
#[test]
fn backlog_removal_cleanup_purge_composes_history_once_and_replays_full_plan() {
    let f = purge_fixture();
    let context = f.command();
    let preview = preview_maintenance(&context, MaintenanceKind::Purge, today()).unwrap();
    let plan = preview.plan.unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert_eq!(
        plan.edits
            .iter()
            .filter(|e| e.relative.ends_with("notes.md"))
            .count(),
        1
    );
    assert_eq!(
        plan.edits
            .iter()
            .filter(|e| e.relative.ends_with(".METADATA.json"))
            .count(),
        1
    );
    let receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    assert_eq!(receipt.removed.len(), 2);
    let notes =
        std::fs::read_to_string(f.workspace.root().join("projects/atlas/notes.md")).unwrap();
    assert_eq!(notes.matches("## Deleted backlog tasks").count(), 1);
    assert!(notes.contains("**T1** Old") && notes.contains("**T2** Older"));
    let metadata: serde_json::Value = serde_json::from_slice(
        &std::fs::read(f.workspace.root().join("projects/atlas/.METADATA.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        metadata["deleted_backlog_tasks"].as_array().unwrap().len(),
        2
    );
    assert_eq!(resume(&context, &receipt.operation_id).unwrap(), receipt);
    assert_eq!(
        std::fs::read_to_string(f.workspace.root().join("projects/atlas/notes.md")).unwrap(),
        notes
    );
}
#[test]
fn backlog_removal_cleanup_notes_changed_or_created_after_preview_stale_consent() {
    for absent in [false, true] {
        let f = purge_fixture();
        let path = f.workspace.root().join("projects/atlas/notes.md");
        if absent {
            std::fs::remove_file(&path).unwrap();
        }
        let plan = preview_maintenance(&f.command(), MaintenanceKind::Purge, today())
            .unwrap()
            .plan
            .unwrap();
        let before = std::fs::read(f.workspace.root().join("tasks/tasks.csv")).unwrap();
        std::fs::write(path, "changed").unwrap();
        assert_eq!(
            apply(&f.command(), &plan, &plan.fingerprint, today())
                .unwrap_err()
                .code,
            "stale_plan"
        );
        assert_eq!(
            std::fs::read(f.workspace.root().join("tasks/tasks.csv")).unwrap(),
            before
        );
    }
}
#[test]
fn backlog_removal_cleanup_ambiguity_skips_all_eligible_twins() {
    let f = WorkspaceFixture::new();
    f.write("tasks/tasks.csv", b"task_id,task_name,status,backlogged_date,created_date\nT1,Same,backlog,2020-01-01,\nT2,Same,not_started,,2020-02-01\nT3,Same,in_progress,,2020-03-01\n");
    let preview = preview_maintenance(&f.command(), MaintenanceKind::Dedupe, today()).unwrap();
    assert!(preview.plan.is_none());
    assert_eq!(preview.skipped[0].survivor_ids, ["T2", "T3"]);
}
#[test]
fn backlog_removal_cleanup_dedupe_substitution_self_cycle_blocks_entire_batch() {
    let f = WorkspaceFixture::new();
    f.write("tasks/tasks.csv", b"task_id,task_name,status,backlogged_date,created_date,blocked_by,last_touched\nT1,Same,backlog,2020-01-01,,,\nT2,Same,not_started,,2020-02-01,T1,\n");
    let plan = preview_maintenance(&f.command(), MaintenanceKind::Dedupe, today())
        .unwrap()
        .plan
        .unwrap();
    assert_eq!(plan.blockers[0].code, "dependency_cycle");
    assert!(plan.edits.is_empty());
}
#[test]
fn backlog_removal_cleanup_ordinary_serialization_keeps_legacy_shape() {
    let f = WorkspaceFixture::new();
    let plan = preview(
        &f.command(),
        &needles(&["T17"]),
        Options::default(),
        today(),
    )
    .unwrap();
    let value = serde_json::to_value(&plan).unwrap();
    assert!(value.get("maintenance").is_none());
    let decoded: crate::tasks::removal::model::RemovalPlan = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, plan);
    let receipt = apply(&f.command(), &plan, &plan.fingerprint, today()).unwrap();
    let operation =
        crate::tasks::removal::operation_status(&f.command(), &receipt.operation_id).unwrap();
    assert!(operation.canonical_inputs.get("maintenance").is_none());
    assert!(operation.canonical_inputs.get("purge_notes").is_none());
    assert_eq!(
        read_csv(&f.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[test]
fn backlog_removal_cleanup_preserves_original_survivor_duplicates_when_coalescing_replacements() {
    let actual = crate::tasks::removal::references::replace_tokens(
        &["T2", "T2", "T1", "other", "other"].map(str::to_owned),
        &std::collections::BTreeMap::from([("T1".into(), "T2".into())]),
        false,
    );
    assert_eq!(actual, ["T2", "T2", "other", "other"]);
}
