
#[test]
fn csv_note_sums_added_merged_deleted_and_flags_soft_conflicts() {
    use crate::sync::csv_sync::CsvMergeOutcome;
    let outcomes = [
        CsvMergeOutcome {
            name: "tasks.csv".into(),
            added: 2,
            deleted: 1,
            merged: 3,
            soft_conflicts: 1,
        },
        CsvMergeOutcome {
            name: "habits.csv".into(),
            added: 1,
            deleted: 0,
            merged: 0,
            soft_conflicts: 0,
        },
    ];
    assert_eq!(format_csv_note(&outcomes), "csv: +3 ~3 -1 (1 soft)");
}

#[test]
fn csv_note_omits_soft_suffix_when_none() {
    use crate::sync::csv_sync::CsvMergeOutcome;
    let outcomes = [CsvMergeOutcome {
        name: "tasks.csv".into(),
        added: 1,
        ..Default::default()
    }];
    assert_eq!(format_csv_note(&outcomes), "csv: +1 ~0 -0");
}

#[test]
fn csv_preflight_failure_skips_counter_reconciliation() {
    use std::cell::Cell;

    let directory = tempfile::tempdir().unwrap();
    let counter = directory.path().join("tasks/.tasks_next_id");
    std::fs::create_dir_all(counter.parent().unwrap()).unwrap();
    std::fs::write(&counter, "11\n").unwrap();
    let counters_called = Cell::new(false);
    let result = sync_task_state(
        || {
            Err(crate::sync::csv_sync::CsvSyncError::Preflight(
                "habits.csv missing task_uuid".to_owned(),
            ))
        },
        |_| {
            counters_called.set(true);
            std::fs::write(&counter, "99\n").unwrap();
        },
    );

    assert!(matches!(
        result,
        Err(crate::sync::csv_sync::CsvSyncError::Preflight(_))
    ));
    assert!(!counters_called.get());
    assert_eq!(std::fs::read_to_string(counter).unwrap(), "11\n");
}

#[test]
fn sync_once_refuses_when_unconfigured() {
    let cfg: SyncConfig = serde_json::from_str("{}").unwrap();
    let paths = crate::workspace::WorkspacePaths::new(
        Path::new("/home/tester"),
        crate::workspace::WorkspaceId::new(),
    );
    let err = sync_once(
        &paths,
        crate::workspace::WorkspaceId::new(),
        &cfg,
        Path::new("/tmp"),
        Direction::Both,
        ("a", "b", "2026-07-25"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("brain sync setup"));
}
