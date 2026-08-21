
#[test]
fn conflict_display_paths_drop_loose_non_parseable_matches() {
    let files = vec![
        crate::sync::conflicts::ConflictFile {
            path: PathBuf::from("idea (conflict mac 2026-07-25).md"),
        },
        crate::sync::conflicts::ConflictFile {
            path: PathBuf::from("not actually (conflict text).md"),
        },
    ];

    assert_eq!(
        conflict_display_paths(&files),
        vec![PathBuf::from("idea (conflict mac 2026-07-25).md")]
    );
}

#[test]
fn hostname_is_nonempty_and_unqualified() {
    let h = hostname();
    assert!(!h.is_empty());
    assert!(!h.contains('.'));
}

#[test]
fn direction_labels_are_stable() {
    assert_eq!(direction_label(Direction::Both), "both");
    assert_eq!(direction_label(Direction::Resync), "resync");
}

#[test]
fn journal_progress_names_the_local_run_record() {
    let line = journal_progress(Theme::dark(false));

    assert!(line.contains("sync journal"), "{line}");
}
