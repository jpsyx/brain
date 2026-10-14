use super::fixture::{WorkspaceFixture, needles};
use crate::tasks::removal::{
    apply, model::Options, operation_status, operations::Phase, preview, resume,
};
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

#[test]
fn removal_completed_resume_uses_original_receipt_despite_later_task_edits() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    let receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    fixture.write("tasks/tasks.csv", b"task_id,task_name\nT17,A later task\n");
    let progress = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{}.json", receipt.operation_id));
    let before = std::fs::read(progress.clone()).unwrap();
    assert_eq!(resume(&context, &receipt.operation_id).unwrap(), receipt);
    assert_eq!(std::fs::read(progress).unwrap(), before);
    assert!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("A later task")
    );
}

#[test]
fn removal_private_immutable_receipt_overrides_forged_progress() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    let receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    let directory = context.workspace.paths().task_removals_dir();
    let progress = directory.join(format!("{}.json", receipt.operation_id));
    let commit = directory.join(format!("{}.commit.json", receipt.operation_id));
    for path in [&progress, &commit] {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let immutable = std::fs::read(&commit).unwrap();
    let mut operation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
    operation["phase"] = serde_json::json!("abandoned");
    operation["receipt"] = serde_json::Value::Null;
    std::fs::write(progress, serde_json::to_vec(&operation).unwrap()).unwrap();
    assert_eq!(
        operation_status(&context, &receipt.operation_id)
            .unwrap()
            .phase,
        Phase::LocalCommitted
    );
    assert_eq!(resume(&context, &receipt.operation_id).unwrap(), receipt);
    assert_eq!(std::fs::read(commit).unwrap(), immutable);
}

#[test]
fn removal_completed_receipt_preserves_derived_warnings_and_canonical_paths() {
    let fixture = WorkspaceFixture::new();
    fixture.project("projects/example");
    let context = fixture.command();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    let mut receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    assert_eq!(
        receipt.updated,
        ["projects/example/.METADATA.json", "tasks/tasks.csv"]
    );
    receipt
        .warnings
        .push(crate::tasks::removal::model::Problem::new(
            "agenda_refresh_failed",
            "Agenda refresh failed after commit",
            serde_json::json!({}),
        ));
    let progress = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{}.json", receipt.operation_id));
    let mut operation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
    operation["receipt"] = serde_json::to_value(&receipt).unwrap();
    std::fs::write(&progress, serde_json::to_vec(&operation).unwrap()).unwrap();
    assert_eq!(resume(&context, &receipt.operation_id).unwrap(), receipt);
}

#[test]
fn removal_uncommitted_progress_receipt_cannot_skip_local_commit() {
    let fixture = WorkspaceFixture::new();
    let id = super::recovery::crash(
        &fixture,
        crate::tasks::store_transaction::TransactionStep::Install(0),
    );
    let context = fixture.command();
    let progress = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.json"));
    let mut operation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
    operation["receipt"] = serde_json::json!({"operation_id":id,"committed":false,"removed":[],"updated":[],"external":[],"warnings":[]});
    std::fs::write(progress, serde_json::to_vec(&operation).unwrap()).unwrap();
    assert!(resume(&context, &id).unwrap().committed);
    assert_eq!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}
