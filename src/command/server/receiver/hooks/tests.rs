include!("tests_support.rs");
include!("tests_sections/portable_commands.rs");
include!("tests_sections/atomic_installation.rs");
include!("tests_sections/lifecycle_delivery.rs");

#[test]
fn an_unchanged_lifecycle_artifact_is_never_rewritten() {
    // Rewriting identical bytes gives the file a new mtime, which trips Brain's
    // own workspace watcher and pushes an unchanged hook script to the remote on
    // every TUI launch.
    assert!(!super::needs_rewrite(
        Some(b"#!/usr/bin/env python3\n"),
        "#!/usr/bin/env python3\n"
    ));
}

#[test]
fn a_changed_or_absent_artifact_is_written() {
    assert!(super::needs_rewrite(None, "payload"));
    assert!(super::needs_rewrite(Some(b"old"), "payload"));
    assert!(super::needs_rewrite(Some(b""), "payload"));
}

#[test]
fn reinstalling_lifecycle_artifacts_leaves_their_mtimes_untouched() {
    // The end-to-end property the phantom startup push came from.
    let home = tempfile::tempdir().expect("home");
    let root = home.path().join("brain");
    std::fs::create_dir_all(&root).expect("root");
    super::install_for_home(&root, home.path()).expect("first install");
    let before = artifact_mtimes(&root);
    assert!(!before.is_empty(), "no lifecycle artifacts were installed");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    super::install_for_home(&root, home.path()).expect("reinstall");

    assert_eq!(
        artifact_mtimes(&root),
        before,
        "reinstalling identical artifacts must not touch any mtime"
    );
}

fn artifact_mtimes(root: &std::path::Path) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                found.push((path, modified));
            }
        }
    }
    found.sort();
    found
}
