use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{FileChange, TransactionStep, journal_path, recover_pending, replace_group_with_hook};

fn fixture() -> (tempfile::TempDir, Vec<FileChange>) {
    let root = tempfile::tempdir().unwrap();
    let changes = ["tasks.csv", "habits.csv", "config.json"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let path = root.path().join(name);
            let before = format!("old-{index}\n").into_bytes();
            std::fs::write(&path, &before).unwrap();
            FileChange {
                path,
                before: Some(before),
                after: format!("new-{index}\n").into_bytes(),
            }
        })
        .collect();
    (root, changes)
}

fn original(changes: &[FileChange]) -> Vec<FileChange> {
    changes.to_vec()
}

fn assert_old(changes: &[FileChange]) {
    for change in changes {
        assert_eq!(
            std::fs::read(&change.path).unwrap(),
            change.before.clone().unwrap()
        );
    }
}

#[test]
fn every_stage_failure_leaves_the_prior_generation() {
    for failure in 0..3 {
        let (root, changes) = fixture();
        let old = original(&changes);
        let result = replace_group_with_hook(root.path(), &changes, |step| {
            if step == TransactionStep::Stage(failure) {
                return Err(io::Error::other("injected stage failure"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_old(&old);
        assert!(!journal_path(root.path()).exists());
    }
}

#[test]
fn every_install_failure_rolls_back_the_prior_generation() {
    for failure in 0..3 {
        let (root, changes) = fixture();
        let old = original(&changes);
        let result = replace_group_with_hook(root.path(), &changes, |step| {
            if step == TransactionStep::Install(failure) {
                return Err(io::Error::other("injected install failure"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_old(&old);
        assert!(!journal_path(root.path()).exists());
    }
}

#[test]
fn interruption_at_every_install_boundary_recovers_the_prior_generation() {
    for failure in 0..3 {
        let (root, changes) = fixture();
        let old = original(&changes);
        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let _ = replace_group_with_hook(root.path(), &changes, |step| {
                assert_ne!(step, TransactionStep::Install(failure), "injected crash");
                Ok(())
            });
        }));
        assert!(interrupted.is_err());
        assert!(journal_path(root.path()).exists());

        recover_pending(root.path()).unwrap();

        assert_old(&old);
        assert!(!journal_path(root.path()).exists());
    }
}

#[test]
fn rollback_failure_is_reported_and_remains_recoverable() {
    let (root, changes) = fixture();
    let old = original(&changes);
    let error = replace_group_with_hook(root.path(), &changes, |step| match step {
        TransactionStep::Install(2) => Err(io::Error::other("injected install failure")),
        TransactionStep::Restore(0) => Err(io::Error::other("injected restore failure")),
        _ => Ok(()),
    })
    .unwrap_err();
    assert!(format!("{error:#}").contains("rollback also failed"));
    assert!(journal_path(root.path()).exists());

    recover_pending(root.path()).unwrap();
    assert_old(&old);
}
