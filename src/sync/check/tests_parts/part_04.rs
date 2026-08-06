
#[test]
fn report_counts_csv_rows_and_shows_csv_summaries() {
    let t = Theme::dark(false);
    let csv = vec![CsvPending {
        name: "tasks.csv".to_string(),
        push: CsvSideDiff {
            added: 2,
            changed: 1,
            deleted: 0,
        },
        pull: Some(CsvSideDiff {
            added: 0,
            changed: 0,
            deleted: 1,
        }),
    }];

    let report = format_report(&[], &[], &csv, t);

    assert!(report.contains("Changes to push (3)"), "{report:?}");
    assert!(report.contains("tasks.csv: +2 ~1 -0 rows"), "{report:?}");
    assert!(report.contains("Changes to pull (1)"), "{report:?}");
    assert!(report.contains("tasks.csv: +0 ~0 -1 rows"), "{report:?}");
    assert!(
        report.contains("Run `brain sync` to push and pull all changes."),
        "{report:?}"
    );
}

#[test]
fn report_explains_csv_deltas_are_baseline_diffs_not_provenance() {
    let t = Theme::dark(false);
    let csv = vec![CsvPending {
        name: "tasks.csv".to_string(),
        push: CsvSideDiff {
            added: 1,
            ..Default::default()
        },
        pull: Some(CsvSideDiff {
            changed: 1,
            ..Default::default()
        }),
    }];

    let report = format_report(&[], &[], &csv, t);

    assert!(
        report.contains("CSV rows are compared against this machine's cached baseline"),
        "{report:?}"
    );
    assert!(
        report.contains("not proof that another machine made the change"),
        "{report:?}"
    );
    assert!(
        report.contains("brain sync will merge tasks.csv/habits.csv by id"),
        "{report:?}"
    );
}

#[test]
fn report_warns_when_remote_csv_was_not_checked() {
    let t = Theme::dark(false);
    let csv = vec![CsvPending {
        name: "tasks.csv".to_string(),
        push: CsvSideDiff::default(),
        pull: None,
    }];

    let report = format_report(&[], &[], &csv, t);

    assert!(
        report.contains("Could not check remote CSV changes for tasks.csv."),
        "{report:?}"
    );
    assert!(!report.contains("In sync"), "{report:?}");
    assert!(!report.contains("Run `brain sync`"), "{report:?}");
}

#[test]
fn collect_csv_pending_reads_baseline_local_and_remote_without_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let local_path = dir.path().join("tasks/tasks.csv");
    std::fs::create_dir_all(local_path.parent().expect("parent")).expect("mkdir");
    let base = "task_id,title,status\n1,base,open\n";
    let local = "task_id,title,status\n1,local,open\n2,local add,open\n";
    let remote = "task_id,title,status\n";
    std::fs::write(&local_path, local).expect("write local");

    let pending = collect_csv_pending_with_fetch(
        dir.path(),
        &["tasks/tasks.csv"],
        |name| {
            assert_eq!(name, "tasks.csv");
            Ok(base.to_string())
        },
        |rel| {
            assert_eq!(rel, "tasks/tasks.csv");
            Some(remote.to_string())
        },
    )
    .unwrap();

    assert_eq!(
        pending,
        vec![CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff {
                added: 1,
                changed: 1,
                deleted: 0
            },
            pull: Some(CsvSideDiff {
                added: 0,
                changed: 0,
                deleted: 1
            }),
        }]
    );
    assert_eq!(
        std::fs::read_to_string(local_path).expect("read local"),
        local
    );
}
