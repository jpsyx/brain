//! Gated integration test: exercises the real `rclone bisync` flow (brain's
//! own argument builder + runner + parser) between two local dirs. Runs only
//! when `rclone` is on PATH, so the default suite passes without rclone.

use std::path::Path;
use std::process::Command;

use brain::sync::args::{bisync_args, Direction};
use brain::sync::config::SyncConfig;
use brain::sync::run::run_rclone;
use brain::sync::verify::{self, Outcome};

fn rclone_available() -> bool {
    Command::new("rclone").arg("version").output().is_ok_and(|o| o.status.success())
}

fn cfg() -> SyncConfig {
    serde_json::from_str(r#"{"enabled":true,"b2_bucket":"unused","max_delete_percent":90}"#).unwrap()
}

fn run(a: &Path, b: &Path, dir: Direction) -> brain::sync::run::RunOutcome {
    let args = bisync_args(&cfg(), &a.to_string_lossy(), &b.to_string_lossy(), dir);
    run_rclone(&[], &args)
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
        std::fs::read_dir(&a).unwrap().filter_map(Result::ok).map(|e| e.file_name()).collect::<Vec<_>>()
    );
    assert_eq!(brain::sync::conflicts::leftover_markers(&a), 0, "no raw markers should remain");

    // Verification must surface the conflict, not report clean.
    match verify::classify(&outcome, renamed, 0) {
        Outcome::NeedsAttention(_) => {}
        other => panic!("expected NeedsAttention for a real conflict, got {other:?}"),
    }

    std::fs::remove_dir_all(&base).ok();
}
