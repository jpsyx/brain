use super::*;

#[test]
fn strip_removes_ansi_and_timestamp_level_prefix() {
    let raw = "\x1b[36m2026/07/25 15:59:55\x1b[0m INFO  : \x1b[32mbrandnew.md\x1b[0m: Copied (server-side copy)";
    assert_eq!(strip(raw), "brandnew.md: Copied (server-side copy)");
}

#[test]
fn strip_with_no_prefix_just_removes_ansi() {
    let raw = "\x1b[32mBisync successful\x1b[0m";
    assert_eq!(strip(raw), "Bisync successful");
}

#[test]
fn strip_trims_trailing_whitespace() {
    let raw = "2026/07/25 15:59:55 NOTICE: Bisync successful   \n";
    assert_eq!(strip(raw), "Bisync successful");
}

#[test]
fn classify_applied_copied_variants_all_map_to_copied_with_path() {
    assert_eq!(
        classify_applied("brandnew.md: Copied (server-side copy)"),
        Some(Applied::Copied("brandnew.md".to_string()))
    );
    assert_eq!(
        classify_applied("notes/one.md: Copied (new)"),
        Some(Applied::Copied("notes/one.md".to_string()))
    );
    assert_eq!(
        classify_applied("changeme.md: Copied (replaced existing)"),
        Some(Applied::Copied("changeme.md".to_string()))
    );
}

#[test]
fn classify_applied_deleted() {
    assert_eq!(
        classify_applied("deleteme.md: Deleted"),
        Some(Applied::Deleted("deleteme.md".to_string()))
    );
}

#[test]
fn classify_applied_progress_stats_line() {
    let line = "32 B / 32 B, 100%, 0 B/s, ETA -";
    assert_eq!(
        classify_applied(line),
        Some(Applied::Progress(line.to_string()))
    );
}

#[test]
fn classify_applied_done() {
    assert_eq!(classify_applied("Bisync successful"), Some(Applied::Done));
}

#[test]
fn classify_applied_abort_max_delete() {
    assert_eq!(
        classify_applied("Safety abort: too many deletes (>50%, 1 of 1)..."),
        Some(Applied::AbortMaxDelete)
    );
}

#[test]
fn classify_applied_abort_prior_listing() {
    assert_eq!(
        classify_applied("Bisync critical error: cannot find prior Path1 or Path2 listings"),
        Some(Applied::AbortPriorListing)
    );
    assert_eq!(
        classify_applied("Bisync aborted. Must run --resync to recover."),
        Some(Applied::AbortPriorListing)
    );
}

#[test]
fn classify_applied_noise_is_none() {
    assert_eq!(classify_applied("Some unrelated log line"), None);
}

#[test]
fn render_applied_copied_plain() {
    let t = crate::theme::Theme::dark(false);
    assert_eq!(
        render_applied(&Applied::Copied("notes/x.md".to_string()), t),
        Some("  ✓ notes/x.md".to_string())
    );
}

#[test]
fn render_applied_deleted_plain() {
    let t = crate::theme::Theme::dark(false);
    assert_eq!(
        render_applied(&Applied::Deleted("notes/x.md".to_string()), t),
        Some("  ✗ notes/x.md (deleted)".to_string())
    );
}

#[test]
fn render_applied_progress_plain() {
    let t = crate::theme::Theme::dark(false);
    let line = "32 B / 32 B, 100%, 0 B/s, ETA -";
    assert_eq!(
        render_applied(&Applied::Progress(line.to_string()), t),
        Some(format!("  {line}"))
    );
}

#[test]
fn render_applied_done_and_aborts_are_none() {
    let t = crate::theme::Theme::dark(false);
    assert_eq!(render_applied(&Applied::Done, t), None);
    assert_eq!(render_applied(&Applied::AbortMaxDelete, t), None);
    assert_eq!(render_applied(&Applied::AbortPriorListing, t), None);
}

#[test]
fn render_applied_copied_colored_contains_success_ansi() {
    let t = crate::theme::Theme::dark(true);
    let rendered = render_applied(&Applied::Copied("notes/x.md".to_string()), t).unwrap();
    assert!(
        rendered.contains("\x1b[92m"),
        "expected green success ANSI in {rendered:?}"
    );
}

#[test]
fn classify_change_path1_file_changed() {
    assert_eq!(
        classify_change("- Path1    File changed: size (larger), time (newer) - resources/r1.md"),
        Some(Change {
            side: Side::Push,
            kind: ChangeKind::Changed,
            path: "resources/r1.md".to_string()
        })
    );
}

#[test]
fn classify_change_path1_file_is_new() {
    assert_eq!(
        classify_change("- Path1    File is new               - notes/n3.md"),
        Some(Change {
            side: Side::Push,
            kind: ChangeKind::New,
            path: "notes/n3.md".to_string()
        })
    );
}

#[test]
fn classify_change_path1_file_was_deleted() {
    assert_eq!(
        classify_change("- Path1    File was deleted          - deleteme.md"),
        Some(Change {
            side: Side::Push,
            kind: ChangeKind::Deleted,
            path: "deleteme.md".to_string()
        })
    );
}

#[test]
fn classify_change_path2_file_is_new() {
    assert_eq!(
        classify_change("- Path2    File is new               - remote-added.md"),
        Some(Change {
            side: Side::Pull,
            kind: ChangeKind::New,
            path: "remote-added.md".to_string()
        })
    );
}

#[test]
fn classify_change_path2_file_was_deleted() {
    assert_eq!(
        classify_change("- Path2    File was deleted          - top.md"),
        Some(Change {
            side: Side::Pull,
            kind: ChangeKind::Deleted,
            path: "top.md".to_string()
        })
    );
}

#[test]
fn classify_change_queue_line_is_none() {
    assert_eq!(classify_change("Queue copy to Path2: notes/n3.md"), None);
    assert_eq!(
        classify_change("- Path1    Queue copy to Path2                - notes/n3.md"),
        None
    );
}

#[test]
fn summarize_groups_by_top_level_dir_count_desc() {
    let paths = vec![
        "notes/n3.md".to_string(),
        "notes/n4.md".to_string(),
        "resources/r1.md".to_string(),
    ];
    assert_eq!(
        summarize(&paths),
        vec![
            "2 changes in notes/".to_string(),
            "1 change in resources/".to_string()
        ]
    );
}

#[test]
fn summarize_top_level_file_named_directly() {
    assert_eq!(
        summarize(&["top.md".to_string()]),
        vec!["1 change to top.md".to_string()]
    );
}

#[test]
fn summarize_mixes_dirs_and_files_sorted_by_count_then_name() {
    let paths = vec![
        "a/x.md".to_string(),
        "a/y.md".to_string(),
        "a/z.md".to_string(),
        "b/one.md".to_string(),
        "solo.md".to_string(),
    ];
    assert_eq!(
        summarize(&paths),
        vec![
            "3 changes in a/".to_string(),
            "1 change in b/".to_string(),
            "1 change to solo.md".to_string()
        ]
    );
}
