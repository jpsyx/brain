use super::*;

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
    let paths = workspace_paths(&base, workspace_id());

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

    std::fs::write(a.join("note.md"), b"hello").unwrap();
    std::fs::write(a.join("keep.md"), b"keep").unwrap();
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");
    assert!(
        b.join("note.md").exists(),
        "create did not propagate A to B"
    );
    assert!(
        b.join("keep.md").exists(),
        "create did not propagate A to B"
    );

    // Leave one file behind because rclone deliberately refuses an empty
    // Path1 listing as protection against an accidentally wiped source.
    std::fs::remove_file(a.join("note.md")).unwrap();
    let deleted = run(&a, &b, Direction::Both);
    assert!(deleted.exit_ok, "delete sync failed: {deleted:?}");
    assert!(
        !b.join("note.md").exists(),
        "delete did not propagate A to B"
    );
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

    for name in ["one.md", "two.md", "three.md"] {
        std::fs::write(a.join(name), b"stable").unwrap();
    }
    let resync = run(&a, &b, Direction::Resync);
    assert!(resync.exit_ok, "resync failed: {resync:?}");
    assert!(b.join("one.md").exists(), "create did not propagate A to B");

    std::fs::create_dir_all(a.join("notes")).unwrap();
    std::fs::rename(a.join("one.md"), a.join("notes").join("one.md")).unwrap();
    let moved = run(&a, &b, Direction::Both);
    assert!(moved.exit_ok, "move sync failed: {moved:?}");
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
