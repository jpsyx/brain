use super::*;

#[test]
fn removal_interruptions_recover_at_every_publication_install_and_cleanup_boundary() {
    let mut boundaries = vec![
        TransactionStep::RemoveJournal,
        TransactionStep::SyncJournalRemoval,
    ];
    for state in [
        JournalState::Preparing,
        JournalState::Prepared,
        JournalState::Committed,
    ] {
        boundaries.extend([
            TransactionStep::PublishJournalWrite(state),
            TransactionStep::PublishJournalRename(state),
            TransactionStep::PublishJournalSync(state),
        ]);
    }
    for index in 0..4 {
        boundaries.extend([
            TransactionStep::Stage(index),
            TransactionStep::Backup(index),
            TransactionStep::Install(index),
            TransactionStep::SyncInstall(index),
            TransactionStep::CleanupStaged(index),
            TransactionStep::CleanupBackup(index),
        ]);
    }
    for boundary in boundaries {
        let (_temporary, workspace, changes) = removal_fixture();
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        crash_at(&workspace, &owner, &changes, boundary);
        recover_pending(&workspace, &owner).unwrap();
        let committed = matches!(
            boundary,
            TransactionStep::CleanupStaged(_)
                | TransactionStep::CleanupBackup(_)
                | TransactionStep::RemoveJournal
                | TransactionStep::SyncJournalRemoval
                | TransactionStep::PublishJournalSync(JournalState::Committed)
        );
        for change in &changes {
            let expected = if committed {
                &change.after
            } else {
                change.before.as_ref().unwrap()
            };
            assert_eq!(
                &std::fs::read(&change.path).unwrap(),
                expected,
                "{boundary:?}"
            );
        }
        assert!(!journal_path(&workspace, TargetPolicy::TaskRemoval).exists());
    }
}

#[test]
fn rollback_interruption_and_cleanup_are_retryable() {
    let mut boundaries = vec![
        TransactionStep::PublishJournalWrite(JournalState::Preparing),
        TransactionStep::PublishJournalRename(JournalState::Preparing),
        TransactionStep::PublishJournalSync(JournalState::Preparing),
        TransactionStep::RemoveJournal,
        TransactionStep::SyncJournalRemoval,
    ];
    for index in 0..4 {
        boundaries.extend([
            TransactionStep::Restore(index),
            TransactionStep::CleanupStaged(index),
            TransactionStep::CleanupBackup(index),
        ]);
    }
    for boundary in boundaries {
        let (_temporary, workspace, changes) = removal_fixture();
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        crash_at(
            &workspace,
            &owner,
            &changes,
            TransactionStep::SyncInstall(2),
        );
        let mut reached = false;
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _ = recover_all_locked(&workspace, &mut |step| {
                if step == boundary {
                    reached = true;
                    panic!("recovery interruption");
                }
                Ok(())
            });
        }));
        assert!(reached, "{boundary:?}");
        recover_pending(&workspace, &owner).unwrap();
        assert_old(&changes);
    }
}

#[test]
fn removal_recovers_legacy_schema_two_before_publishing_new_journal() {
    let (_temporary, workspace, changes) = fixture();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = replace_group_with_hook(
            &workspace,
            &owner,
            TargetPolicy::ManagedTriage,
            &changes,
            |step| {
                assert_ne!(step, TransactionStep::SyncInstall(0), "legacy crash");
                Ok(())
            },
        );
    }));
    let path = workspace
        .root()
        .join(".config/.brain-triage-habits-transaction.json");
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(journal["schema_version"], 2);
    assert!(journal.get("policy").is_none());
    let mut reached = false;
    replace_group_with_hook(
        &workspace,
        &owner,
        TargetPolicy::TaskRemoval,
        &changes[..1],
        |step| {
            if step == TransactionStep::PublishJournalWrite(JournalState::Preparing)
                && !path.exists()
            {
                reached = true;
                assert_old(&changes);
                assert!(!path.exists());
            }
            Ok(())
        },
    )
    .unwrap();
    assert!(reached);
}
