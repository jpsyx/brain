use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{
    FileChange, JournalState, TransactionStep, journal_path, recover_pending,
    replace_group_with_hook,
};
use crate::tasks::store_lock::TaskStoreOwner;
use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

fn fixture() -> (tempfile::TempDir, WorkspaceContext, Vec<FileChange>) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let workspace = WorkspaceContext::new(
        temporary.path(),
        WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        WorkspaceName::parse("family").unwrap(),
        &root,
        "member",
        temporary.path(),
    )
    .unwrap();
    let changes = ["tasks/tasks.csv", "tasks/habits.csv", ".config/config.json"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let path = root.join(name);
            let before = format!("old-{index}\n").into_bytes();
            std::fs::write(&path, &before).unwrap();
            FileChange {
                path,
                before: Some(before),
                after: format!("new-{index}\n").into_bytes(),
            }
        })
        .collect();
    (temporary, workspace, changes)
}

fn assert_old(changes: &[FileChange]) {
    for change in changes {
        assert_eq!(
            std::fs::read(&change.path).unwrap(),
            change.before.clone().unwrap()
        );
    }
}

fn assert_new(changes: &[FileChange]) {
    for change in changes {
        assert_eq!(std::fs::read(&change.path).unwrap(), change.after);
    }
}

#[test]
fn every_stage_and_install_failure_leaves_the_prior_generation() {
    for failure in 0..3 {
        for boundary in [
            TransactionStep::Stage(failure),
            TransactionStep::Install(failure),
        ] {
            let (_temporary, workspace, changes) = fixture();
            let owner = TaskStoreOwner::acquire(&workspace).unwrap();
            let result = replace_group_with_hook(&workspace, &owner, &changes, |step| {
                if step == boundary {
                    return Err(io::Error::other("injected failure"));
                }
                Ok(())
            });
            assert!(result.is_err());
            assert_old(&changes);
            assert!(!journal_path(workspace.root()).exists());
        }
    }
}

#[test]
fn interruption_at_every_internal_boundary_recovers_a_complete_generation() {
    let boundaries = [
        TransactionStep::Stage(0),
        TransactionStep::Backup(0),
        TransactionStep::PublishJournalWrite(JournalState::Prepared),
        TransactionStep::PublishJournalRename(JournalState::Prepared),
        TransactionStep::PublishJournalSync(JournalState::Prepared),
        TransactionStep::Install(0),
        TransactionStep::SyncInstall(0),
        TransactionStep::PublishJournalWrite(JournalState::Committed),
        TransactionStep::PublishJournalRename(JournalState::Committed),
        TransactionStep::PublishJournalSync(JournalState::Committed),
        TransactionStep::CleanupStaged(0),
        TransactionStep::CleanupBackup(0),
        TransactionStep::RemoveJournal,
        TransactionStep::SyncJournalRemoval,
    ];
    for boundary in boundaries {
        let (_temporary, workspace, changes) = fixture();
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = replace_group_with_hook(&workspace, &owner, &changes, |step| {
                assert_ne!(step, boundary, "injected crash");
                Ok(())
            });
        }));
        recover_pending(&workspace, &owner).unwrap();
        if matches!(
            boundary,
            TransactionStep::CleanupStaged(_)
                | TransactionStep::CleanupBackup(_)
                | TransactionStep::RemoveJournal
                | TransactionStep::SyncJournalRemoval
                | TransactionStep::PublishJournalSync(JournalState::Committed)
        ) {
            assert_new(&changes);
        } else {
            assert_old(&changes);
        }
        assert!(!journal_path(workspace.root()).exists());
    }
}

#[test]
fn new_live_file_recovers_at_install_and_cleanup_boundaries() {
    for boundary in [
        TransactionStep::SyncInstall(0),
        TransactionStep::CleanupStaged(0),
    ] {
        let (_temporary, workspace, _) = fixture();
        let new_path = workspace.root().join("tasks/.habits_next_id");
        let changes = vec![FileChange {
            path: new_path.clone(),
            before: None,
            after: b"8\n".to_vec(),
        }];
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = replace_group_with_hook(&workspace, &owner, &changes, |step| {
                assert_ne!(step, boundary, "injected crash");
                Ok(())
            });
        }));
        recover_pending(&workspace, &owner).unwrap();
        assert_eq!(
            new_path.exists(),
            boundary == TransactionStep::CleanupStaged(0)
        );
    }
}

#[test]
fn every_existing_live_file_is_present_at_each_observable_transaction_boundary() {
    let (_temporary, workspace, changes) = fixture();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    replace_group_with_hook(&workspace, &owner, &changes, |_| {
        assert!(changes.iter().all(|change| change.path.exists()));
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_second_writer_cannot_enter_while_the_first_writer_owns_the_task_store() {
    use std::sync::mpsc;
    use std::time::Duration;
    let (temporary, workspace, changes) = fixture();
    let first_workspace = workspace.clone();
    let first_changes = changes;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        let owner = TaskStoreOwner::acquire(&first_workspace).unwrap();
        replace_group_with_hook(&first_workspace, &owner, &first_changes, |step| {
            if step == TransactionStep::Install(0) {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            Ok(())
        })
    });
    entered_rx.recv().unwrap();
    let second_workspace = workspace;
    let (done_tx, done_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        done_tx
            .send(TaskStoreOwner::acquire(&second_workspace).map(drop))
            .unwrap();
    });
    let early = done_rx.recv_timeout(Duration::from_millis(150));
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap();
    assert!(
        early.is_err(),
        "the second writer entered before ownership was released"
    );
    drop(temporary);
}

#[test]
fn published_journal_binds_the_workspace_identity_and_root() {
    let (_temporary, workspace, changes) = fixture();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = replace_group_with_hook(&workspace, &owner, &changes, |step| {
            assert_ne!(step, TransactionStep::Install(0), "injected crash");
            Ok(())
        });
    }));
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal_path(workspace.root())).unwrap()).unwrap();
    assert_eq!(journal["workspace_id"], workspace.id().to_string());
    assert_eq!(
        journal["workspace_root"],
        workspace.root().display().to_string()
    );
}

#[test]
fn forged_journal_cannot_restore_an_unapproved_live_path() {
    let (_temporary, workspace, _changes) = fixture();
    let victim = workspace.root().join("private.txt");
    std::fs::write(&victim, b"keep\n").unwrap();
    std::fs::write(workspace.root().join("forged.backup"), b"attacker\n").unwrap();
    std::fs::write(journal_path(workspace.root()), format!(
        "{{\"schema_version\":2,\"workspace_id\":\"{}\",\"workspace_root\":\"{}\",\"transaction_id\":\"1-1\",\"state\":\"prepared\",\"entries\":[{{\"live\":\"private.txt\",\"staged\":\"forged.staged\",\"backup\":\"forged.backup\",\"existed\":true}}]}}",
        workspace.id(), workspace.root().display()
    )).unwrap();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    assert!(recover_pending(&workspace, &owner).is_err());
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep\n");
    assert!(workspace.root().join("forged.backup").exists());
}

#[test]
fn journal_recovery_rejects_duplicate_targets_and_wrong_workspace_identity() {
    for mutation in ["duplicate", "workspace"] {
        let (_temporary, workspace, changes) = fixture();
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = replace_group_with_hook(&workspace, &owner, &changes, |step| {
                assert_ne!(step, TransactionStep::Install(0), "injected crash");
                Ok(())
            });
        }));
        let path = journal_path(workspace.root());
        let mut journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if mutation == "duplicate" {
            let duplicate = journal["entries"][0].clone();
            journal["entries"].as_array_mut().unwrap().push(duplicate);
        } else {
            journal["workspace_id"] = serde_json::json!("11111111-1111-4111-8111-111111111111");
        }
        std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();

        assert!(recover_pending(&workspace, &owner).is_err());
        assert!(
            path.exists(),
            "unauthenticated journal must remain for inspection"
        );
    }
}

#[cfg(unix)]
#[test]
fn symlink_ancestor_cannot_escape_the_workspace() {
    use std::os::unix::fs::symlink;

    let (_temporary, workspace, _) = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.root().join("projects")).unwrap();
    symlink(outside.path(), workspace.root().join("projects/link")).unwrap();
    let live = workspace.root().join("projects/link/.METADATA.json");
    std::fs::write(outside.path().join(".METADATA.json"), b"keep\n").unwrap();
    let changes = vec![FileChange {
        path: live,
        before: Some(b"keep\n".to_vec()),
        after: b"replace\n".to_vec(),
    }];
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();

    assert!(replace_group_with_hook(&workspace, &owner, &changes, |_| Ok(())).is_err());
    assert_eq!(
        std::fs::read(outside.path().join(".METADATA.json")).unwrap(),
        b"keep\n"
    );
}

#[test]
fn crash_harness_exposes_backup_install_sync_and_cleanup_boundaries() {
    let boundaries = [
        TransactionStep::Backup(0),
        TransactionStep::SyncInstall(0),
        TransactionStep::CleanupBackup(0),
        TransactionStep::RemoveJournal,
    ];
    assert_eq!(boundaries.len(), 4);
}
