use super::OperationOwner;
use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

#[test]
fn nonblocking_owner_refuses_live_lock_and_reacquires_after_release() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let workspace = WorkspaceContext::new(
        temporary.path(),
        WorkspaceId::new(),
        WorkspaceName::parse("example").unwrap(),
        &root,
        "member",
        temporary.path(),
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let owner = OperationOwner::acquire(&workspace, &id).unwrap();
    assert!(
        OperationOwner::try_acquire(&workspace, &id)
            .unwrap()
            .is_none()
    );
    drop(owner);
    assert!(
        OperationOwner::try_acquire(&workspace, &id)
            .unwrap()
            .is_some()
    );
}
