use super::*;

/// An unconfigured sync block: the remote lane stays off, so the local-side
/// fs tests below never reach for rclone or the network.
fn local_only() -> SyncConfig {
    SyncConfig::default()
}

fn idea_conflict_files() -> Vec<ConflictFile> {
    vec![
        ConflictFile {
            path: PathBuf::from("idea (conflict mac 2026-07-25).md"),
        },
        ConflictFile {
            path: PathBuf::from("idea (conflict server 2026-07-24).md"),
        },
        ConflictFile {
            path: PathBuf::from("other (conflict mac 2026-07-25).md"),
        },
    ]
}

#[test]
fn the_remote_lane_runs_whenever_the_canonical_exists_even_with_no_local_copies() {
    // The local copy may already be gone (resolved by an older brain that
    // only cleaned the local side), while the remote loser still lingers.
    assert!(remote_lane_applies(&ResolveDecision::NoCopies));
    assert!(remote_lane_applies(&ResolveDecision::Delete(vec![
        PathBuf::from("idea (conflict mac 2026-07-25).md")
    ])));
    assert!(
        !remote_lane_applies(&ResolveDecision::CanonicalMissing),
        "a missing canonical refuses outright; never touch the remote"
    );
}

#[test]
fn no_copies_summary_reports_a_remote_only_cleanup() {
    let remote = RemoteResolution {
        deleted: vec![PathBuf::from("idea.md.__brainconflict__1")],
        failed: vec![],
        listed: true,
    };
    let line = no_copies_summary("idea.md", Some(&remote), Theme::dark(false));
    assert!(line.contains("idea.md"), "{line}");
    assert!(
        line.contains("1 remote object"),
        "a remote-only cleanup must still be reported: {line}"
    );
}

#[test]
fn no_copies_summary_keeps_the_plain_message_when_both_sides_are_clean() {
    let remote = RemoteResolution {
        deleted: vec![],
        failed: vec![],
        listed: true,
    };
    assert_eq!(
        no_copies_summary("idea.md", Some(&remote), Theme::dark(false)),
        "no conflict copies for idea.md"
    );
    assert_eq!(
        no_copies_summary("idea.md", None, Theme::dark(false)),
        "no conflict copies for idea.md"
    );
}

#[test]
fn should_clean_remote_requires_both_a_configured_remote_and_rclone() {
    assert!(should_clean_remote(true, true));
    assert!(!should_clean_remote(false, true));
    assert!(!should_clean_remote(true, false));
}

#[test]
fn summary_reports_the_remote_objects_it_deleted() {
    let remote = RemoteResolution {
        deleted: vec![PathBuf::from("idea.md.__brainconflict__1")],
        failed: vec![],
        listed: true,
    };
    let line = resolve_summary("idea.md", 1, Some(&remote), Theme::dark(false));
    assert!(line.contains("removed 1 copy"), "{line}");
    assert!(
        line.contains("1 remote object"),
        "the remote deletion must be visible in the summary: {line}"
    );
}

#[test]
fn summary_says_the_remote_was_clean_when_nothing_was_there() {
    let remote = RemoteResolution {
        deleted: vec![],
        failed: vec![],
        listed: true,
    };
    let line = resolve_summary("idea.md", 1, Some(&remote), Theme::dark(false));
    assert!(
        !line.contains("remote object"),
        "no remote losers means no remote count to report: {line}"
    );
    assert!(!line.contains("could not"), "{line}");
}

#[test]
fn summary_admits_when_the_remote_could_not_be_checked() {
    let remote = RemoteResolution::default(); // listed: false
    let line = resolve_summary("idea.md", 1, Some(&remote), Theme::dark(false));
    assert!(
        line.contains("could not check the remote"),
        "an unreachable remote must never read as a clean remote: {line}"
    );
}

#[test]
fn summary_stays_local_only_when_the_remote_lane_did_not_run() {
    let line = resolve_summary("idea.md", 2, None, Theme::dark(false));
    assert_eq!(line, "resolved idea.md (removed 2 copies)");
}

#[test]
fn resolve_decision_refuses_when_canonical_is_missing() {
    let files = idea_conflict_files();
    assert_eq!(
        resolve_decision(Path::new("idea.md"), false, &files),
        ResolveDecision::CanonicalMissing
    );
}

#[test]
fn resolve_decision_deletes_copies_when_canonical_present() {
    let files = idea_conflict_files();
    assert_eq!(
        resolve_decision(Path::new("idea.md"), true, &files),
        ResolveDecision::Delete(vec![
            PathBuf::from("idea (conflict mac 2026-07-25).md"),
            PathBuf::from("idea (conflict server 2026-07-24).md"),
        ])
    );
}

#[test]
fn resolve_decision_reports_no_copies_when_canonical_present_but_unmatched() {
    let files = idea_conflict_files();
    assert_eq!(
        resolve_decision(Path::new("nope.md"), true, &files),
        ResolveDecision::NoCopies
    );
}

#[test]
fn resolve_decision_prefers_canonical_missing_over_no_copies() {
    // Guard precedence: a bogus original with no copies AND a missing
    // canonical is still refused, not silently treated as a no-op.
    let files = idea_conflict_files();
    assert_eq!(
        resolve_decision(Path::new("nope.md"), false, &files),
        ResolveDecision::CanonicalMissing
    );
}

#[test]
fn resolve_deletes_the_copy_and_keeps_the_canonical() {
    // Hermetic fs check: real canonical + real conflict copy in a temp
    // dir, no rclone involved.
    let tmp = std::env::temp_dir().join(format!("brain-resolve-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("idea.md"), b"canonical").unwrap();
    fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

    resolve(&tmp, &local_only(), &["idea.md".to_owned()]).unwrap();

    assert!(
        tmp.join("idea.md").exists(),
        "canonical must survive resolve"
    );
    assert!(
        !tmp.join("idea (conflict mac 2026-07-25).md").exists(),
        "the conflict copy must be deleted"
    );

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn resolve_deletes_copies_for_multiple_originals_in_one_call() {
    let tmp = std::env::temp_dir().join(format!("brain-resolve-many-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("idea.md"), b"merged idea").unwrap();
    fs::write(tmp.join("other.md"), b"merged other").unwrap();
    fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"idea loser").unwrap();
    fs::write(
        tmp.join("other (conflict mac 2026-07-25).md"),
        b"other loser",
    )
    .unwrap();

    resolve(
        &tmp,
        &local_only(),
        &["idea.md".to_owned(), "other.md".to_owned()],
    )
    .unwrap();

    assert!(
        tmp.join("idea.md").exists(),
        "first canonical must survive resolve"
    );
    assert!(
        tmp.join("other.md").exists(),
        "second canonical must survive resolve"
    );
    assert!(
        !tmp.join("idea (conflict mac 2026-07-25).md").exists(),
        "first conflict copy must be deleted"
    );
    assert!(
        !tmp.join("other (conflict mac 2026-07-25).md").exists(),
        "second conflict copy must be deleted"
    );

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn resolve_deletes_nested_subdir_conflict_copy() {
    let tmp = std::env::temp_dir().join(format!("brain-resolve-nested-{}", std::process::id()));
    let dir = tmp.join("projects");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("idea.md"), b"merged").unwrap();
    fs::write(dir.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

    resolve(&tmp, &local_only(), &["projects/idea.md".to_owned()]).unwrap();

    assert!(
        dir.join("idea.md").exists(),
        "nested canonical must survive resolve"
    );
    assert!(
        !dir.join("idea (conflict mac 2026-07-25).md").exists(),
        "nested conflict copy must be deleted"
    );

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn resolve_leaves_everything_when_canonical_is_missing() {
    let tmp = std::env::temp_dir().join(format!("brain-resolve-missing-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

    resolve(&tmp, &local_only(), &["idea.md".to_owned()]).unwrap();

    assert!(
        tmp.join("idea (conflict mac 2026-07-25).md").exists(),
        "must refuse to delete when the canonical original is missing"
    );

    fs::remove_dir_all(&tmp).ok();
}
