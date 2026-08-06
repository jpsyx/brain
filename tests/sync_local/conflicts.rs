use super::*;

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

    for name in ["one", "two", "three", "four"] {
        std::fs::write(a.join(format!("{name}.md")), format!("orig-{name}")).unwrap();
    }
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");

    std::fs::write(a.join("one.md"), "A-side-change").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(b.join("one.md"), "B-side-change-different").unwrap();

    let outcome = run(&a, &b, Direction::Both);
    assert!(outcome.exit_ok, "conflict bisync failed: {outcome:?}");

    let renamed = brain::sync::conflicts::rename_markers(&a, "testhost", "2026-07-25");
    assert_eq!(renamed, 1, "expected exactly one conflict copy renamed");
    assert!(
        a.join("one (conflict testhost 2026-07-25).md").exists(),
        "friendly conflict file not found; dir: {:?}",
        std::fs::read_dir(&a)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        brain::sync::conflicts::leftover_markers(&a),
        0,
        "no raw markers should remain"
    );

    match verify::classify(&outcome, renamed, 0) {
        Outcome::NeedsAttention(_) => {}
        other => panic!("expected NeedsAttention for a real conflict, got {other:?}"),
    }

    std::fs::remove_dir_all(&base).ok();
}

/// Proves a real rclone-generated conflict is grouped and resolved through the
/// production conflict surfaces while the merged canonical file survives.
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

    for name in ["one", "two", "three"] {
        std::fs::write(a.join(format!("{name}.md")), format!("orig-{name}")).unwrap();
    }
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");

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
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>()
    );

    let files = brain::sync::conflicts::list_conflicts(&a);
    let groups = brain::sync::conflicts::group_conflicts(&files);
    assert_eq!(
        groups.len(),
        1,
        "expected exactly one conflict group, got {groups:?}"
    );
    let group = &groups[0];
    assert_eq!(group.original, Path::new("one.md"));
    assert_eq!(group.copies.len(), 1, "expected one copy for one.md");
    assert_eq!(group.copies[0].host, host);
    assert_eq!(group.copies[0].date, date);

    std::fs::write(a.join("one.md"), "merged: A-side + B-side").unwrap();
    brain::sync::command::resolve(&a, &["one.md".to_string()]).unwrap();

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
    assert!(a.join("two.md").exists());
    assert!(a.join("three.md").exists());

    std::fs::remove_dir_all(&base).ok();
}
