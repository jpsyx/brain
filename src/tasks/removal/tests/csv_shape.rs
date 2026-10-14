use super::fixture::{WorkspaceFixture, needles, today};
use crate::tasks::removal::{model::Options, snapshot::Snapshot};

fn rejects_shape_without_changing_bytes(header: &str, records: &str) {
    for (relative, prefix) in [("tasks/tasks.csv", "T"), ("tasks/habits.csv", "H")] {
        let fixture = WorkspaceFixture::new();
        let bytes = format!("{header}\n{}", records.replace("{id}", prefix));
        fixture.write(relative, bytes.as_bytes());
        let task_before = std::fs::read(fixture.workspace.root().join("tasks/tasks.csv")).unwrap();
        let habit_before =
            std::fs::read(fixture.workspace.root().join("tasks/habits.csv")).unwrap();
        let problem = Snapshot::read(&fixture.context(), today()).expect_err("invalid CSV shape");
        assert_eq!(problem.code, "invalid_task_csv");
        assert_eq!(problem.details["path"], relative);
        assert_eq!(
            std::fs::read(fixture.workspace.root().join("tasks/tasks.csv")).unwrap(),
            task_before
        );
        assert_eq!(
            std::fs::read(fixture.workspace.root().join("tasks/habits.csv")).unwrap(),
            habit_before
        );
    }
}

#[test]
fn snapshot_csv_shape_rejects_duplicate_headers_without_discarding_cells() {
    rejects_shape_without_changing_bytes(
        "task_id,task_name,note,note",
        "{id}17,Send proposal,,\n{id}2,Other,first,second\n",
    );
}

#[test]
fn snapshot_csv_shape_rejects_over_width_records_without_discarding_cells() {
    rejects_shape_without_changing_bytes(
        "task_id,task_name,note",
        "{id}17,Send proposal,\n{id}2,Other,first,second\n",
    );
}

#[test]
fn snapshot_csv_shape_preserves_short_legacy_record_defaults_during_planning() {
    let fixture = WorkspaceFixture::new();
    let bytes = b"task_id,task_name,note\nT17,Send proposal,anchor\nT2,Other\n";
    fixture.write("tasks/tasks.csv", bytes);
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_eq!(snapshot.tasks.parsed.rows[1]["note"], "");
    let plan =
        crate::tasks::removal::plan::plan(&snapshot, &needles(&["T17"]), &Options::default());
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].before.as_deref(), Some(bytes.as_slice()));
    assert_eq!(plan.edits[0].after, b"task_id,task_name,note\nT2,Other,\n");
    assert_eq!(
        std::fs::read(fixture.workspace.root().join("tasks/tasks.csv")).unwrap(),
        bytes
    );
}
