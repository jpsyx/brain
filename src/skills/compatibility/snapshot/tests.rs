use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[test]
fn absent_optional_directory_is_empty_but_discovered_disappearance_fails() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        CheckedSources::capture(Some(&root.path().join("absent")))
            .unwrap()
            .projection
            .sources
            .is_empty()
    );
    assert!(
        CheckedSources::read_selected(root.path(), vec![root.path().join("vanished.md")]).is_err()
    );
}

#[test]
fn invalid_unreadable_and_nonregular_extensions_fail_closed() {
    for kind in ["invalid", "unreadable", "directory", "symlink"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("agenda.md");
        match kind {
            "invalid" => fs::write(&source, b"\xff").unwrap(),
            "unreadable" => {
                fs::write(&source, "CUSTOM").unwrap();
                fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
            }
            "directory" => fs::create_dir(&source).unwrap(),
            _ => {
                fs::write(root.path().join("target"), "CUSTOM").unwrap();
                symlink(root.path().join("target"), &source).unwrap();
            }
        }
        assert!(
            CheckedSources::capture(Some(root.path())).is_err(),
            "{kind}"
        );
    }
}

#[test]
fn symlinked_source_directory_and_parent_fail() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    symlink(root.path().join("real"), root.path().join("alias")).unwrap();
    assert!(CheckedSources::capture(Some(&root.path().join("alias"))).is_err());
    assert!(CheckedSources::capture(Some(&root.path().join("alias/extensions"))).is_err());
}

#[test]
fn checked_sources_preserve_registered_and_sandbox_identities() {
    let root = tempfile::tempdir().unwrap();
    for prefix in [".config/extensions", "extensions"] {
        let directory = root.path().join(prefix);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("todo.md"), "[todo:anchors]\nA\n").unwrap();
        let checked = CheckedSources::capture(Some(&directory)).unwrap();
        assert_eq!(
            checked.projection.sources[&format!("{prefix}/todo.md")],
            b"[todo:anchors]\nA\n"
        );
    }
}
