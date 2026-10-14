use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
mod authentication;
mod legacy;
mod recovery;
use legacy::{assert_old, fixture};

fn removal_fixture() -> (tempfile::TempDir, WorkspaceContext, Vec<FileChange>) {
    let (temporary, workspace, mut changes) = fixture();
    changes.truncate(2);
    for name in [
        "archive/projects/work__sample/.METADATA.json",
        "projects/work__sample/notes.md",
    ] {
        let path = workspace.root().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"original metadata").unwrap();
        changes.push(FileChange {
            path,
            before: Some(b"original metadata".to_vec()),
            after: b"removed task metadata".to_vec(),
        });
    }
    (temporary, workspace, changes)
}

fn crash_at(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    changes: &[FileChange],
    boundary: TransactionStep,
) {
    let mut reached = false;
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let result = replace_group_with_hook(
            workspace,
            owner,
            TargetPolicy::TaskRemoval,
            changes,
            |step| {
                if step == boundary {
                    reached = true;
                    panic!("injected crash");
                }
                Ok(())
            },
        );
        result.unwrap();
    }));
    assert!(reached, "boundary {boundary:?} not reached");
    assert!(panic.is_err());
}

#[test]
fn removal_group_recovers_after_csv_replacement() {
    let (_temporary, workspace, changes) = removal_fixture();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    let mut reached = false;
    let result = replace_group_with_hook(
        &workspace,
        &owner,
        TargetPolicy::TaskRemoval,
        &changes,
        |step| {
            if step == TransactionStep::SyncInstall(0) {
                reached = true;
                return Err(io::Error::other("after CSV replacement"));
            }
            Ok(())
        },
    );
    assert!(
        reached,
        "removal transaction must reach CSV replacement: {result:?}"
    );
    assert!(result.is_err());
    recover_pending(&workspace, &owner).unwrap();
    assert_old(&changes);
}

mod writers;

mod migration_writers;
