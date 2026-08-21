#[test]
fn in_sync_when_nothing_to_push_or_pull() {
    let t = Theme::dark(false);
    let report = format_report(&[], &[], &[], t);
    assert!(report.contains("In sync"), "{report:?}");
    assert!(!report.contains("brain sync"), "{report:?}");
}

#[test]
fn push_only_reports_count_summary_and_push_suggestion() {
    let t = Theme::dark(false);
    let push = vec!["notes/a.md".to_string(), "notes/b.md".to_string()];
    let report = format_report(&push, &[], &[], t);
    assert!(report.contains("Changes to push (2)"), "{report:?}");
    assert!(report.contains("2 changes in notes/"), "{report:?}");
    assert!(
        report.contains("Run `brain sync` to push your changes."),
        "{report:?}"
    );
}

#[test]
fn pull_only_reports_pull_suggestion() {
    let t = Theme::dark(false);
    let pull = vec!["remote-added.md".to_string()];
    let report = format_report(&[], &pull, &[], t);
    assert!(report.contains("Changes to pull (1)"), "{report:?}");
    assert!(
        report.contains("Run `brain sync` to pull the latest changes."),
        "{report:?}"
    );
}

#[test]
fn both_sides_report_push_and_pull_suggestion() {
    let t = Theme::dark(false);
    let push = vec!["a.md".to_string()];
    let pull = vec!["b.md".to_string()];
    let report = format_report(&push, &pull, &[], t);
    assert!(report.contains("Changes to push (1)"), "{report:?}");
    assert!(report.contains("Changes to pull (1)"), "{report:?}");
    assert!(
        report.contains("Run `brain sync` to push and pull all changes."),
        "{report:?}"
    );
}
