use std::path::Path;

use brain::sync::csv_sync::baseline_path;
use brain::sync::journal::{Journal, SyncRun};
use brain::workspace::{WorkspaceId, WorkspacePaths};

fn paths(home: &Path, id: &str) -> WorkspacePaths {
    WorkspacePaths::new(home, WorkspaceId::parse(id).expect("valid workspace id"))
}

fn personal_paths(home: &Path) -> WorkspacePaths {
    paths(home, "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
}

fn family_paths(home: &Path) -> WorkspacePaths {
    paths(home, "e806258e-491a-436d-9db4-a5ca9903e0d4")
}

fn run(note: &str) -> SyncRun {
    SyncRun {
        started_at: "2026-08-05T00:00:00Z".to_owned(),
        finished_at: "2026-08-05T00:00:01Z".to_owned(),
        direction: "both".to_owned(),
        outcome: "clean".to_owned(),
        transferred: 1,
        deleted: 0,
        conflicts: 0,
        errors: 0,
        note: note.to_owned(),
    }
}

#[test]
fn every_sync_runtime_path_is_separated_by_workspace_uuid() {
    let home = tempfile::tempdir().expect("temporary home");
    let personal = personal_paths(home.path());
    let family = family_paths(home.path());

    assert_ne!(personal.sync_dir(), family.sync_dir());
    assert_ne!(personal.sync_lock(), family.sync_lock());
    assert_ne!(personal.sync_journal(), family.sync_journal());
    assert_ne!(personal.sync_current_state(), family.sync_current_state());
    assert_ne!(personal.sync_current_log(), family.sync_current_log());
    assert_ne!(
        brain::sync::run::bisync_workdir(&personal),
        brain::sync::run::bisync_workdir(&family)
    );
    assert_ne!(
        baseline_path(&personal, "tasks.csv"),
        baseline_path(&family, "tasks.csv")
    );
    assert_eq!(
        personal.sync_dir(),
        home.path()
            .join(".cache/brain/workspaces")
            .join("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
            .join("sync")
    );
}

#[test]
fn different_workspace_locks_coexist_while_the_same_uuid_stays_serialized() {
    let home = tempfile::tempdir().expect("temporary home");
    let personal = personal_paths(home.path());
    let family = family_paths(home.path());

    let personal_guard =
        brain::sync::lock::try_acquire(&personal.sync_lock()).expect("personal lock");
    let family_guard = brain::sync::lock::try_acquire(&family.sync_lock()).expect("family lock");

    assert!(
        brain::sync::lock::try_acquire(&personal.sync_lock()).is_none(),
        "a second sync for the same UUID must be rejected"
    );
    drop(family_guard);
    drop(personal_guard);
}

#[test]
fn journal_rows_cannot_be_read_through_another_workspace_path() {
    let home = tempfile::tempdir().expect("temporary home");
    let personal = personal_paths(home.path());
    let family = family_paths(home.path());
    let family_journal = Journal::open(&family.sync_journal()).expect("family journal");
    family_journal
        .record(&run("family-only"))
        .expect("family journal row");

    let personal_rows = Journal::open(&personal.sync_journal())
        .expect("personal journal")
        .recent(10)
        .expect("personal rows");
    let family_rows = family_journal.recent(10).expect("family rows");

    assert!(personal_rows.is_empty());
    assert_eq!(family_rows.len(), 1);
    assert_eq!(family_rows[0].note, "family-only");
}

#[test]
fn current_state_cannot_be_read_through_another_workspace_path() {
    let home = tempfile::tempdir().expect("temporary home");
    let personal = personal_paths(home.path());
    let family = family_paths(home.path());
    let reporter = brain::sync::current::Reporter::begin(
        &family,
        "pull",
        "2026-08-05T00:00:00Z",
        std::process::id(),
    );

    assert!(brain::sync::current::read_state(&personal).is_none());
    assert_eq!(
        brain::sync::current::read_state(&family)
            .expect("family current state")
            .direction,
        "pull"
    );
    drop(reporter);
}
