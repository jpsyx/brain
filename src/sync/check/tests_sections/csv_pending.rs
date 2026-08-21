
#[test]
fn csv_pending_tracks_push_and_pull_sides_independently() {
    let base = "task_id,title,status\n1,base,open\n";
    let local = "task_id,title,status\n1,local,open\n2,local add,open\n";
    let remote = "task_id,title,status\n";

    assert_eq!(
        csv_pending_from_texts(
            "tasks/tasks.csv",
            base,
            local,
            Some(remote),
            SchemaStatus::Legacy,
        )
        .unwrap(),
        CsvPending {
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
        }
    );
}

#[test]
fn csv_pending_with_missing_baseline_does_not_double_count_identical_sides() {
    let csv = "task_id,title,status\n1,same,open\n2,also same,open\n";

    assert_eq!(
        csv_pending_from_texts("tasks/tasks.csv", "", csv, Some(csv), SchemaStatus::Legacy,)
            .unwrap(),
        CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff::default(),
            pull: Some(CsvSideDiff::default()),
        }
    );
}

#[test]
fn csv_pending_with_missing_baseline_treats_remote_as_provisional_snapshot() {
    let remote = "task_id,title,status\n1,old,open\n";
    let local = "task_id,title,status\n1,old,open\n2,new local,open\n";

    assert_eq!(
        csv_pending_from_texts(
            "tasks/tasks.csv",
            "",
            local,
            Some(remote),
            SchemaStatus::Legacy,
        )
        .unwrap(),
        CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff {
                added: 1,
                changed: 0,
                deleted: 0,
            },
            pull: Some(CsvSideDiff::default()),
        }
    );
}

#[test]
fn csv_pending_with_missing_baseline_and_empty_local_reports_pull_only() {
    let remote = "task_id,title,status\n1,remote,open\n";
    let local = "task_id,title,status\n";

    assert_eq!(
        csv_pending_from_texts(
            "tasks/tasks.csv",
            "",
            local,
            Some(remote),
            SchemaStatus::Legacy,
        )
        .unwrap(),
        CsvPending {
            name: "tasks.csv".to_string(),
            push: CsvSideDiff::default(),
            pull: Some(CsvSideDiff {
                added: 1,
                changed: 0,
                deleted: 0,
            }),
        }
    );
}
