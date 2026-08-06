use super::*;

/// Real `rclone bisync -v` output for a run that resynced 5 new files:
/// note the TWO `Transferred:` lines (bytes, then file count) — only the
/// file-count one (`5 / 5, 100%`) should be picked up.
#[test]
fn parses_real_transferred_two_line_format() {
    let out = "Transferred:   \t         50 B / 50 B, 100%, 0 B/s, ETA -\n\
                    Checks:                 0 / 0, -, Listed 10\n\
                    Transferred:            5 / 5, 100%\n\
                    Server Side Copies:     5 @ 50 B\n\
                    Elapsed time:         0.0s\n";
    let o = parse_outcome(true, out);
    assert_eq!(o.transferred, 5);
}

/// Real output for a delete-only run: no file-count `Transferred:` line
/// is printed at all (rclone omits it when nothing transferred), and
/// `Deleted:` carries a `(files), (dirs), (freed)` breakdown.
#[test]
fn parses_real_deleted_only_format() {
    let out = "Transferred:   \t          0 B / 0 B, -, 0 B/s, ETA -\n\
                    Checks:                10 / 10, 100%, Listed 6\n\
                    Deleted:                1 (files), 0 (dirs), 10 B (freed)\n\
                    Elapsed time:         0.0s\n";
    let o = parse_outcome(true, out);
    assert_eq!(o.transferred, 0);
    assert_eq!(o.deleted, 1);
}

/// Real output for a run that hit fatal errors building listings.
#[test]
fn parses_real_errors_format() {
    let out = "Transferred:   \t          0 B / 0 B, -, 0 B/s, ETA -\n\
                    Errors:                 2 (fatal error encountered)\n\
                    Checks:                 2 / 2, 100%, Listed 0\n\
                    Elapsed time:         0.0s\n";
    let o = parse_outcome(false, out);
    assert_eq!(o.errors, 2);
}

#[test]
fn detects_max_delete_abort() {
    // Real wording: rclone's safety-abort message says "too many
    // deletes", never the literal flag name "--max-delete" or "max
    // delete".
    let o = parse_outcome(
        false,
        "ERROR : Safety abort: too many deletes (>50%, 1 of 1) on Path1 \"/a/\". Run with --force if desired.\n\
             NOTICE: Bisync aborted. Please try again.\n\
             NOTICE: Failed to bisync: too many deletes\n",
    );
    assert_eq!(o.abort, Some(AbortKind::MaxDelete));
}

#[test]
fn detects_prior_listing_missing() {
    // Real wording captured from `rclone bisync` against a path with no
    // prior baseline listings.
    let o = parse_outcome(
        false,
        "ERROR : Bisync critical error: cannot find prior Path1 or Path2 listings, likely due to critical error on prior run\n\
             ERROR : Bisync aborted. Must run --resync to recover.\n\
             NOTICE: Failed to bisync: bisync aborted\n",
    );
    assert_eq!(o.abort, Some(AbortKind::PriorListingMissing));
}

#[test]
fn detects_check_access_abort_before_generic_resync_text() {
    let o = parse_outcome(
        false,
        "NOTICE: --check-access: Failed to find any files named RCLONE_TEST\n\
             ERROR : Access test failed: Path1 count 0, Path2 count 0 - RCLONE_TEST\n\
             ERROR : Bisync critical error: check file check failed\n\
             ERROR : Bisync aborted. Must run --resync to recover.\n\
             NOTICE: Failed to bisync: bisync aborted\n",
    );
    assert_eq!(o.abort, Some(AbortKind::CheckAccess));
}

#[test]
fn unknown_nonzero_exit_is_other_not_clean() {
    assert_eq!(
        parse_outcome(false, "something went wrong").abort,
        Some(AbortKind::Other)
    );
}

#[test]
fn bisync_workdir_is_under_cache_brain_sync() {
    let paths = crate::workspace::WorkspacePaths::new(
        Path::new("/home/tester"),
        crate::workspace::WorkspaceId::new(),
    );
    assert!(bisync_workdir(&paths).ends_with("sync/bisync"));
}

#[test]
fn reaping_removes_leftover_lock_files_but_keeps_listings() {
    // Under brain's own sync lock, any rclone bisync lock file is from a
    // dead interrupted run, so reaping it is always safe. Listing state
    // (the `.lst` baselines) must survive so a normal run can still resume.
    let dir = std::env::temp_dir().join(format!("brain-reap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lck = dir.join("home_p_brain..lck");
    let lst = dir.join("home_p_brain..path1.lst");
    std::fs::write(&lck, "pid 123").unwrap();
    std::fs::write(&lst, "listing").unwrap();

    reap_stale_bisync_locks(&dir);

    assert!(!lck.exists(), "stale .lck must be removed");
    assert!(lst.exists(), "baseline .lst must be preserved");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn reaping_a_missing_workdir_is_a_noop() {
    // First-ever run: the workdir doesn't exist yet. Must not error.
    reap_stale_bisync_locks(&std::env::temp_dir().join("brain-reap-does-not-exist-xyz"));
}

#[test]
fn stale_lock_file_from_an_interrupted_run_routes_to_resync_recovery() {
    // A run killed mid-flight (TUI quit / power off) leaves rclone's bisync
    // lock file behind. The next run must self-heal via resync, not surface
    // a dead-end error the user has to reason about.
    let o = parse_outcome(
        false,
        "ERROR : Bisync critical error: prior lock file found: /Users/p/.cache/brain/sync/bisync/xxx.lck\n\
             ERROR : Bisync aborted. Must run --resync to recover.\n\
             NOTICE: Failed to bisync: bisync aborted\n",
    );
    assert_eq!(o.abort, Some(AbortKind::PriorListingMissing));
}

#[test]
fn a_generic_bisync_critical_error_routes_to_resync_recovery() {
    // Two overlapping runs corrupt the listing files; rclone reports a
    // generic bisync critical error. Treat the interrupted-baseline family
    // as recoverable-by-resync rather than an opaque `Other`.
    let o = parse_outcome(
        false,
        "ERROR : Bisync critical error: failed to read prior listing\n\
             ERROR : Bisync aborted. Must run --resync to recover.\n",
    );
    assert_eq!(o.abort, Some(AbortKind::PriorListingMissing));
}

#[test]
fn a_plain_auth_failure_stays_other_and_does_not_trigger_a_resync() {
    // A credential/network failure is not a baseline problem; resyncing
    // would be pointless and expensive, so it must stay `Other`.
    let o = parse_outcome(
        false,
        "ERROR : Failed to create file system for \"BRAIN:bucket\": 401 unauthorized\n",
    );
    assert_eq!(o.abort, Some(AbortKind::Other));
}
