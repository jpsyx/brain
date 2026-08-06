//! Gated integration test: exercises the real `rclone bisync` flow (brain's
//! own argument builder + runner + parser) between two local dirs. Runs only
//! when `rclone` is on PATH, so the default suite passes without rclone.

use std::path::Path;
use std::process::Command;

use brain::sync::args::{Direction, bisync_args};
use brain::sync::config::SyncConfig;
use brain::sync::current::Reporter;
use brain::sync::remote::Remote;
use brain::sync::run::run_rclone;
use brain::sync::verify::{self, Outcome};

fn rclone_available() -> bool {
    Command::new("rclone")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn cfg() -> SyncConfig {
    serde_json::from_str(r#"{"enabled":true,"b2_bucket":"unused","max_delete_percent":90}"#)
        .unwrap()
}

fn workspace_paths(base: &Path) -> brain::workspace::WorkspacePaths {
    brain::workspace::WorkspacePaths::new(
        base,
        brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
            .expect("valid workspace id"),
    )
}

fn run(a: &Path, b: &Path, dir: Direction) -> brain::sync::run::RunOutcome {
    if dir == Direction::Resync {
        if !brain::workspace::WorkspaceManifest::path(a).exists() {
            let manifest = brain::workspace::WorkspaceManifest::new(workspace_id());
            manifest.write_new(a).unwrap();
            let remote_manifest = brain::workspace::WorkspaceManifest::path(b);
            std::fs::create_dir_all(remote_manifest.parent().unwrap()).unwrap();
            std::fs::copy(
                brain::workspace::WorkspaceManifest::path(a),
                remote_manifest,
            )
            .unwrap();
        }
        let remote = Remote {
            env: Vec::new(),
            arg: b.to_string_lossy().into_owned(),
        };
        let verified = brain::sync::identity::require_remote_identity(
            a,
            workspace_id(),
            &remote,
        )
        .unwrap();
        brain::sync::check_access::ensure_markers(a, &verified).unwrap();
    }
    let parent = a.parent().unwrap();
    let paths = workspace_paths(parent);
    let workdir = brain::sync::run::bisync_workdir(&paths);
    std::fs::create_dir_all(&workdir).ok();
    let args = bisync_args(
        &cfg(),
        &a.to_string_lossy(),
        &b.to_string_lossy(),
        &workdir.to_string_lossy(),
        dir,
    );
    let reporter = Reporter::begin(&paths, "both", "t", std::process::id());
    run_rclone(&reporter, &[], &args)
}

fn workspace_id() -> brain::workspace::WorkspaceId {
    brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("valid workspace id")
}

#[test]
fn local_rclone_populates_the_uuid_scoped_production_workdir() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = std::env::temp_dir().join(format!(
        "brain-sync-workspace-path-it-{}",
        std::process::id()
    ));
    let a = base.join("a");
    let b = base.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(a.join("note.md"), b"hello").unwrap();
    let paths = workspace_paths(&base);

    let outcome = run(&a, &b, Direction::Resync);

    assert!(outcome.exit_ok, "resync failed: {outcome:?}");
    assert!(
        brain::sync::run::bisync_workdir(&paths).exists(),
        "the local-rclone path must populate the selected workspace's production workdir"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn create_and_delete_propagate_bidirectionally() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = std::env::temp_dir().join(format!("brain-sync-it-{}", std::process::id()));
    let a = base.join("a");
    let b = base.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    // Seed two files on side A (see note below on why not one), then
    // establish the baseline.
    std::fs::write(a.join("note.md"), b"hello").unwrap();
    std::fs::write(a.join("keep.md"), b"keep").unwrap();
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");
    assert!(b.join("note.md").exists(), "create did not propagate A→B");
    assert!(b.join("keep.md").exists(), "create did not propagate A→B");

    // Delete one file on A (leaving `keep.md` behind — deleting the *last*
    // remaining file would empty Path1 entirely, and rclone bisync refuses
    // that by design: "Empty current Path1 listing. Cannot sync to an empty
    // directory", aborting with `Must run --resync to recover.` even though
    // nothing is actually wrong. That's an intentional rclone safety guard
    // against an accidentally-wiped source, distinct from the
    // `--max-delete` percentage guard brain already sets; it is not a gap in
    // brain's `bisync_args`, so this test avoids that edge rather than
    // asserting rclone should behave otherwise).
    std::fs::remove_file(a.join("note.md")).unwrap();
    let del = run(&a, &b, Direction::Both);
    assert!(del.exit_ok, "delete sync failed: {del:?}");
    assert!(!b.join("note.md").exists(), "delete did not propagate A→B");
    assert!(b.join("keep.md").exists(), "unrelated file should survive");

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn a_moved_file_propagates_to_its_new_location() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = std::env::temp_dir().join(format!("brain-sync-move-{}", std::process::id()));
    let a = base.join("a");
    let b = base.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    // Seed a few files and establish the baseline.
    for name in ["one.md", "two.md", "three.md"] {
        std::fs::write(a.join(name), b"stable").unwrap();
    }
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");
    assert!(b.join("one.md").exists(), "create did not propagate A→B");

    // Move one.md into a new subdir (same content, new path). bisync sees this
    // as delete-old-path + create-new-path; both must propagate so the file ends
    // up at the new path on B and is gone from the old path.
    std::fs::create_dir_all(a.join("notes")).unwrap();
    std::fs::rename(a.join("one.md"), a.join("notes").join("one.md")).unwrap();
    let mv = run(&a, &b, Direction::Both);
    assert!(mv.exit_ok, "move sync failed: {mv:?}");
    assert!(
        !b.join("one.md").exists(),
        "move did not remove the old path on B"
    );
    assert!(
        b.join("notes").join("one.md").exists(),
        "move did not create the new path on B"
    );
    assert!(
        b.join("two.md").exists() && b.join("three.md").exists(),
        "unrelated files must survive the move"
    );

    std::fs::remove_dir_all(&base).ok();
}

/// Drives the CSV 3-way merge orchestration ([`csv_sync::sync_one`]) over a
/// LOCAL fake remote (plain files) — no rclone, no B2. Local adds task A, the
/// remote adds task B; after one sync both sides hold the union, a baseline is
/// written, and a second sync is a no-op (convergent + idempotent).
#[test]
fn csv_sync_one_converges_local_and_remote_and_is_idempotent() {
    use brain::sync::csv_sync::{baseline_path, sync_one};
    use std::cell::Cell;

    let base = std::env::temp_dir().join(format!("brain-csv-it-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let local = base.join("local.csv");
    let remote = base.join("remote.csv");
    let paths = brain::workspace::WorkspacePaths::new(&base, brain::workspace::WorkspaceId::new());

    let rel = "tasks/tasks.csv";
    let name = Path::new(rel)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let baseline = baseline_path(&paths, &name);
    std::fs::remove_file(&baseline).ok(); // start from no baseline

    let header = "task_id,status,notes,last_touched\n";
    std::fs::write(&local, format!("{header}A,open,alpha,t1\n")).unwrap();
    std::fs::write(&remote, format!("{header}B,open,beta,t1\n")).unwrap();
    let pushes = Cell::new(0);

    let out = sync_one(
        &paths,
        &local,
        rel,
        || std::fs::read_to_string(&remote).ok(),
        |txt| {
            pushes.set(pushes.get() + 1);
            std::fs::write(&remote, txt).is_ok()
        },
    );
    assert_eq!(out.added, 2, "A and B are both new");
    assert_eq!(out.soft_conflicts, 0, "disjoint adds don't conflict");

    let merged = std::fs::read_to_string(&local).unwrap();
    assert_eq!(
        merged,
        std::fs::read_to_string(&remote).unwrap(),
        "local and remote converge"
    );
    assert!(
        merged.contains("A,open,alpha") && merged.contains("B,open,beta"),
        "merged holds the union of both sides: {merged}"
    );
    assert!(baseline.exists(), "baseline snapshot written");

    // Second run: local == remote == baseline already, so nothing changes.
    let out2 = sync_one(
        &paths,
        &local,
        rel,
        || std::fs::read_to_string(&remote).ok(),
        |txt| {
            pushes.set(pushes.get() + 1);
            std::fs::write(&remote, txt).is_ok()
        },
    );
    assert_eq!(out2.added, 0, "idempotent: nothing new on the second run");
    assert_eq!(
        merged,
        std::fs::read_to_string(&local).unwrap(),
        "local unchanged"
    );
    assert_eq!(
        merged,
        std::fs::read_to_string(&remote).unwrap(),
        "remote unchanged"
    );
    assert_eq!(
        pushes.get(),
        1,
        "an unchanged second pass must not rewrite the remote and re-arm the watcher"
    );

    std::fs::remove_dir_all(&base).ok();
    std::fs::remove_file(&baseline).ok();
}

#[test]
fn same_file_conflict_is_renamed_and_surfaced() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = std::env::temp_dir().join(format!("brain-sync-conflict-it-{}", std::process::id()));
    let a = base.join("a");
    let b = base.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    // Seed 4 files and establish the baseline.
    for f in ["one", "two", "three", "four"] {
        std::fs::write(a.join(format!("{f}.md")), format!("orig-{f}")).unwrap();
    }
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");

    // Edit the SAME file on both sides with different content + different
    // mtimes (sleep so rclone's `--conflict-resolve newer` sees a real skew),
    // producing a same-file conflict rclone can't auto-resolve to one winner.
    std::fs::write(a.join("one.md"), "A-side-change").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(b.join("one.md"), "B-side-change-different").unwrap();

    let outcome = run(&a, &b, Direction::Both);
    assert!(outcome.exit_ok, "conflict bisync failed: {outcome:?}");

    // brain's post-pass renames the raw marker to the friendly name.
    let renamed = brain::sync::conflicts::rename_markers(&a, "testhost", "2026-07-25");
    assert_eq!(renamed, 1, "expected exactly one conflict copy renamed");
    assert!(
        a.join("one (conflict testhost 2026-07-25).md").exists(),
        "friendly conflict file not found; dir: {:?}",
        std::fs::read_dir(&a)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        brain::sync::conflicts::leftover_markers(&a),
        0,
        "no raw markers should remain"
    );

    // Verification must surface the conflict, not report clean.
    match verify::classify(&outcome, renamed, 0) {
        Outcome::NeedsAttention(_) => {}
        other => panic!("expected NeedsAttention for a real conflict, got {other:?}"),
    }

    std::fs::remove_dir_all(&base).ok();
}

/// C5 end-to-end resolve round-trip.
///
/// Beat 1 (edit/add/delete propagate A→B) is proven by
/// `create_and_delete_propagate_bidirectionally`; beat 2 (the two CSVs merge
/// with no `(conflict …)` copy, idempotently) is proven by
/// `csv_sync_one_converges_local_and_remote_and_is_idempotent`. THIS test
/// proves beat 3: a real rclone-generated conflict is enumerated via the C5
/// grouping surface (`list_conflicts`/`group_conflicts`) and then resolved via
/// the real `brain sync resolve` command (`command::resolve`), leaving only
/// the (merged) canonical behind.
#[test]
fn conflict_copy_is_enumerated_and_resolved_leaving_only_the_canonical() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let base = std::env::temp_dir().join(format!("brain-sync-resolve-it-{}", std::process::id()));
    let a = base.join("a");
    let b = base.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    // Seed a few files and establish the baseline (recipe from
    // `same_file_conflict_is_renamed_and_surfaced`).
    for f in ["one", "two", "three"] {
        std::fs::write(a.join(format!("{f}.md")), format!("orig-{f}")).unwrap();
    }
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");

    // Edit the SAME file on both sides with different content + a real mtime
    // skew, producing a same-file conflict rclone can't auto-resolve to one
    // winner.
    std::fs::write(a.join("one.md"), "A-side-change").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(b.join("one.md"), "B-side-change-different").unwrap();

    let outcome = run(&a, &b, Direction::Both);
    assert!(outcome.exit_ok, "conflict bisync failed: {outcome:?}");

    let host = "testhost";
    let date = "2026-07-25";
    let renamed = brain::sync::conflicts::rename_markers(&a, host, date);
    assert_eq!(renamed, 1, "expected exactly one conflict copy renamed");
    assert!(
        a.join("one (conflict testhost 2026-07-25).md").exists(),
        "friendly conflict file not found; dir: {:?}",
        std::fs::read_dir(&a)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect::<Vec<_>>()
    );

    // Enumerate via the C5 grouping surface: this is what the resolve skill
    // actually consumes, so prove it sees the real rclone conflict.
    let files = brain::sync::conflicts::list_conflicts(&a);
    let groups = brain::sync::conflicts::group_conflicts(&files);
    assert_eq!(
        groups.len(),
        1,
        "expected exactly one conflict group, got {groups:?}"
    );
    let group = &groups[0];
    assert_eq!(group.original, Path::new("one.md"));
    assert_eq!(
        group.copies.len(),
        1,
        "expected exactly one copy for one.md"
    );
    assert_eq!(group.copies[0].host, host);
    assert_eq!(group.copies[0].date, date);

    // Simulate the agent's merge: overwrite the canonical (the bisync winner
    // already sitting at a/one.md) with the merged result.
    std::fs::write(a.join("one.md"), "merged: A-side + B-side").unwrap();

    // Resolve via the real command: deletes the conflict copy, never touches
    // the canonical, runs no sync.
    brain::sync::command::resolve(&a, &["one.md".to_string()]).unwrap();

    // End state: canonical survives with the merged content, the friendly
    // copy is gone, and no markers/conflicts remain.
    assert_eq!(
        std::fs::read_to_string(a.join("one.md")).unwrap(),
        "merged: A-side + B-side",
        "canonical must survive resolve with the merged content"
    );
    assert!(
        !a.join("one (conflict testhost 2026-07-25).md").exists(),
        "the conflict copy must be deleted by resolve"
    );
    assert!(
        brain::sync::conflicts::list_conflicts(&a).is_empty(),
        "no open conflicts should remain"
    );
    assert_eq!(
        brain::sync::conflicts::leftover_markers(&a),
        0,
        "no raw markers should remain"
    );

    // Unrelated seed files survive untouched.
    assert!(a.join("two.md").exists());
    assert!(a.join("three.md").exists());

    std::fs::remove_dir_all(&base).ok();
}
