use super::fixture::{WorkspaceFixture, needles};
use crate::tasks::removal::{
    abandon, model::Options, operation_status, operations::Phase, preview, resume,
};
use crate::tasks::store_transaction::{JournalState, TransactionStep};

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

pub(super) fn crash(fixture: &WorkspaceFixture, step: TransactionStep) -> String {
    let context = fixture.command();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    let mut stopped = false;
    let error = crate::tasks::removal::execute::apply_with_hook(
        &context,
        &plan,
        &plan.fingerprint,
        today(),
        |current| {
            if current == step {
                stopped = true;
            }
            if stopped {
                Err(std::io::Error::other("simulated crash"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    error.details["operation_id"].as_str().unwrap().to_owned()
}

#[test]
fn removal_resume_recovers_before_commit_and_is_idempotent() {
    let fixture = WorkspaceFixture::new();
    let id = crash(
        &fixture,
        TransactionStep::PublishJournalWrite(JournalState::Committed),
    );
    let context = fixture.command();
    let receipt = resume(&context, &id).unwrap();
    assert!(receipt.committed);
    assert_eq!(receipt.removed[0].task_id, "T17");
    assert_eq!(
        operation_status(&context, &id).unwrap().phase,
        Phase::Complete
    );
    let before = std::fs::read(context.workspace.root().join("tasks/tasks.csv")).unwrap();
    assert_eq!(resume(&context, &id).unwrap(), receipt);
    assert_eq!(
        std::fs::read(context.workspace.root().join("tasks/tasks.csv")).unwrap(),
        before
    );
    assert_eq!(
        abandon(&context, &id).unwrap_err().code,
        "already_committed"
    );
}

#[test]
fn removal_recovery_publishes_receipt_before_removing_committed_journal() {
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    let context = fixture.command();
    let journal = context
        .workspace
        .paths()
        .cache_dir()
        .join("task-removal-transaction.json");
    assert!(journal.exists());
    let before = std::fs::read(&journal).unwrap();
    let progress = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.json"));
    let original = std::fs::read(&progress).unwrap();
    assert_eq!(
        operation_status(&context, &id).unwrap_err().code,
        "recovery_required"
    );
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        before,
        "status must not recover"
    );
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace).unwrap();
    drop(owner);
    assert!(!journal.exists());
    assert_eq!(
        std::fs::read(&progress).unwrap(),
        original,
        "recovery cannot write mutable operation progress"
    );
    let status = operation_status(&context, &id).unwrap();
    assert_eq!(status.phase, Phase::LocalCommitted);
    assert!(status.receipt.unwrap().committed);
    assert!(resume(&context, &id).unwrap().committed);
}

#[test]
fn removal_operation_scope_unavailable_and_abandon_are_safe() {
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::Install(0));
    let context = fixture.command();
    for bad in ["../bad", "13746cfe-053a-4534-8851-462046c5df88"] {
        assert_eq!(
            operation_status(&context, bad).unwrap_err().code,
            "operation_unavailable"
        );
    }
    let other = WorkspaceFixture::new();
    let path = other.workspace.paths().task_removals_dir();
    std::fs::create_dir_all(&path).unwrap();
    let source = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.json"));
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source).unwrap()).unwrap();
    value["workspace_uuid"] = serde_json::json!("13746cfe-053a-4534-8851-462046c5df88");
    std::fs::write(
        path.join(format!("{id}.json")),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert_eq!(
        operation_status(&other.command(), &id).unwrap_err().code,
        "operation_unavailable"
    );
    assert_eq!(abandon(&context, &id).unwrap().phase, Phase::Abandoned);
    assert_eq!(
        resume(&context, &id).unwrap_err().code,
        "operation_abandoned"
    );
    assert_eq!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn removal_recovery_does_not_wait_for_operation_owner_held_by_resume() {
    use std::{sync::mpsc, time::Duration};
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    let context = fixture.command();
    let task_owner =
        crate::tasks::store_lock::TaskStoreOwner::acquire_inspection(&context.workspace).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let worker_context = context.clone();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || {
        done_tx.send(resume(&worker_context, &worker_id)).unwrap();
    });
    let lock = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.lock"));
    let observer = rusqlite::Connection::open(lock).unwrap();
    observer.busy_timeout(Duration::ZERO).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match observer.execute_batch("BEGIN IMMEDIATE") {
            Ok(()) => {
                observer.execute_batch("ROLLBACK").unwrap();
            }
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::DatabaseBusy =>
            {
                break;
            }
            Err(error) => panic!("unexpected lock error: {error}"),
        }
        assert!(
            std::time::Instant::now() < deadline,
            "resume never acquired operation ownership"
        );
        std::thread::yield_now();
    }
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
    // If recovery tried to acquire the operation lock this would deadlock.
    task_owner.recover(&context.workspace).unwrap();
    assert!(operation_status(&context, &id).unwrap().receipt.is_some());
    drop(task_owner);
    assert!(
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap()
            .committed
    );
    worker.join().unwrap();
    assert!(resume(&context, &id).unwrap().committed);
}

#[test]
fn removal_resume_marks_reconfirmation_when_inputs_change() {
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::Install(0));
    let context = fixture.command();
    drop(crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace).unwrap());
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name\nT17,Changed consent\n",
    );
    assert_eq!(resume(&context, &id).unwrap_err().code, "stale_plan");
    assert_eq!(
        operation_status(&context, &id).unwrap().phase,
        Phase::NeedsReconfirmation
    );
    assert!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("Changed consent")
    );
}

#[test]
fn removal_preview_leaves_valid_prepared_journal_and_partial_generation_untouched() {
    let fixture = WorkspaceFixture::new();
    let id = crash(
        &fixture,
        TransactionStep::PublishJournalWrite(JournalState::Committed),
    );
    let context = fixture.command();
    let csv = context.workspace.root().join("tasks/tasks.csv");
    let journal = context
        .workspace
        .paths()
        .cache_dir()
        .join("task-removal-transaction.json");
    let before_csv = std::fs::read(&csv).unwrap();
    let before_journal = std::fs::read(&journal).unwrap();
    assert_eq!(
        preview(&context, &needles(&["T17"]), Options::default(), today())
            .unwrap_err()
            .code,
        "recovery_required"
    );
    assert_eq!(std::fs::read(&csv).unwrap(), before_csv);
    assert_eq!(std::fs::read(&journal).unwrap(), before_journal);
    assert_eq!(
        operation_status(&context, &id).unwrap().phase,
        Phase::Prepared
    );
}

#[test]
fn removal_resume_requires_reconfirmation_after_date_rollover() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let yesterday = today().pred_opt().unwrap();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), yesterday).unwrap();
    let failure = crate::tasks::removal::execute::apply_with_hook(
        &context,
        &plan,
        &plan.fingerprint,
        yesterday,
        |_| Err(std::io::Error::other("simulated crash")),
    )
    .unwrap_err();
    let id = failure.details["operation_id"].as_str().unwrap();
    assert_eq!(resume(&context, id).unwrap_err().code, "stale_plan");
    assert_eq!(
        operation_status(&context, id).unwrap().phase,
        Phase::NeedsReconfirmation
    );
    assert!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("T17")
    );
}

#[test]
fn removal_resume_returns_commit_when_recovery_cleanup_fails() {
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    let context = fixture.command();
    let journal = context
        .workspace
        .paths()
        .cache_dir()
        .join("task-removal-transaction.json");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal).unwrap()).unwrap();
    let backup = context
        .workspace
        .root()
        .join(value["entries"][0]["backup"].as_str().unwrap());
    std::fs::remove_file(&backup).unwrap();
    std::fs::create_dir(&backup).unwrap();
    let receipt = resume(&context, &id).unwrap();
    assert!(receipt.committed);
    assert_eq!(receipt.removed[0].task_id, "T17");
    assert_eq!(
        abandon(&context, &id).unwrap_err().code,
        "already_committed"
    );
}

#[test]
fn downgrade_only_settles_committed_local_journal_and_preserves_progress() {
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    let progress = fixture
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.json"));
    let before = std::fs::read(&progress).unwrap();
    let owner = crate::tasks::removal::prepare_downgrade(&fixture.workspace).unwrap();
    assert_eq!(std::fs::read(&progress).unwrap(), before);
    assert!(
        crate::tasks::removal::operations::load_commit(&fixture.workspace, &id)
            .unwrap()
            .is_some()
    );
    assert!(
        !fixture
            .workspace
            .paths()
            .cache_dir()
            .join("task-removal-transaction.json")
            .exists()
    );
    drop(owner);
    assert!(crate::tasks::removal::prepare_downgrade(&fixture.workspace).is_ok());
}
#[test]
fn downgrade_rejects_unresolved_or_active_removal_without_running_adapters() {
    let fixture = WorkspaceFixture::new();
    let id = crash(&fixture, TransactionStep::Install(0));
    let operation_owner =
        crate::tasks::removal::operations::OperationOwner::acquire(&fixture.workspace, &id)
            .unwrap();
    assert!(crate::tasks::removal::prepare_downgrade(&fixture.workspace).is_err());
    drop(operation_owner);
    assert!(crate::tasks::removal::prepare_downgrade(&fixture.workspace).is_err());
    assert!(
        std::fs::read_to_string(fixture.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("T17")
    );
    assert!(
        crate::tasks::removal::operations::load_commit(&fixture.workspace, &id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn downgrade_detects_running_operation_before_its_first_progress_record() {
    let fixture = WorkspaceFixture::new();
    let id = uuid::Uuid::new_v4().to_string();
    let owner = crate::tasks::removal::operations::OperationOwner::acquire(&fixture.workspace, &id)
        .unwrap();
    assert!(crate::tasks::removal::prepare_downgrade(&fixture.workspace).is_err());
    drop(owner);
    assert!(crate::tasks::removal::prepare_downgrade(&fixture.workspace).is_ok());
}

#[test]
fn interrupted_removal_distinguishes_absent_and_durable_operations() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let absent = crate::tasks::removal::interrupted(&context, &uuid::Uuid::new_v4().to_string())
        .unwrap_err();
    assert_eq!(absent.code, "worker_failed");
    assert!(absent.details.get("operation_id").is_none());
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace).unwrap();
    drop(owner);
    let receipt = crate::tasks::removal::interrupted(&context, &id).unwrap();
    assert!(receipt.committed);
    assert_eq!(receipt.operation_id, id);
}
