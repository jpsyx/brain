use super::*;

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

#[test]
fn interrupted_removal_rejects_contradictory_fingerprints() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    drop(crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace).unwrap());
    let mut operation = operation_status(&context, &id).unwrap();
    operation.fingerprint = "inconsistent-restoration".into();
    operation
        .plan
        .fingerprint
        .clone_from(&operation.fingerprint);
    let owner = crate::tasks::removal::operations::OperationOwner::acquire(&context.workspace, &id)
        .unwrap();
    owner.save(&context.workspace, &operation).unwrap();
    drop(owner);
    assert!(
        operation_status(&context, &id)
            .unwrap_err()
            .message
            .contains("fingerprints disagree")
    );
    let error = crate::tasks::removal::interrupted(&context, &id).unwrap_err();
    assert_eq!(error.code, "outcome_unknown");
    assert_eq!(error.details["operation_id"], id);
    assert!(error.message.contains("fingerprints disagree"));
}

#[test]
fn interrupted_removal_accepts_receipt_only_when_operation_is_proven_missing() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let id = crash(&fixture, TransactionStep::PublishReceipt);
    drop(crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace).unwrap());
    let expected = operation_status(&context, &id).unwrap().receipt.unwrap();
    let path = context
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.json"));
    std::fs::write(&path, b"invalid operation record").unwrap();
    let error = crate::tasks::removal::interrupted(&context, &id).unwrap_err();
    assert_eq!(error.code, "outcome_unknown");
    assert_eq!(error.details["operation_id"], id);
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        crate::tasks::removal::interrupted(&context, &id).unwrap(),
        expected
    );
}
