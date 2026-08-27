#[test]
fn cleanup_fence_downgrade_reserves_the_writer_before_reading_schema() {
    let temp = tempfile::TempDir::new().expect("temporary state directory");
    let path = temp.path().join("state.db");
    drop(Db::open_path(&path).expect("current receiver state"));
    super::super::schema::down_unavailable_notice_path(&path)
        .expect("stage adjacent v10 receiver state");

    let mut blocker = rusqlite::Connection::open(&path).expect("blocking connection");
    let blocker_transaction = blocker
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("reserve blocking writer");
    blocker_transaction
        .execute_batch(
            "ALTER TABLE receiver_jobs RENAME COLUMN recovery_cleanup_session_id
               TO concurrent_cleanup_session_id;
             INSERT OR REPLACE INTO meta (key, value)
               VALUES ('cleanup-down-race', 'held');",
        )
        .expect("hold concurrent cleanup schema change");

    let (event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
    *SCHEMA_RACE_EVENTS
        .lock()
        .expect("install schema race sender") = Some(event_sender.clone());
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let result = super::super::schema::down_cleanup_fence_path_with_busy_observer(
            &worker_path,
            report_schema_busy,
        )
        .map_err(|error| error.to_string());
        event_sender
            .send(SchemaRaceEvent::Finished(result))
            .expect("report cleanup-fence downgrade result");
    });

    assert!(matches!(
        event_receiver
            .recv()
            .expect("first cleanup-fence downgrade event"),
        SchemaRaceEvent::Busy
    ));
    *SCHEMA_RACE_EVENTS
        .lock()
        .expect("clear schema race sender") = None;
    blocker_transaction
        .commit()
        .expect("release blocking writer");
    let result = loop {
        if let SchemaRaceEvent::Finished(result) = event_receiver
            .recv()
            .expect("cleanup-fence downgrade result after wait")
        {
            break result;
        }
    };
    worker.join().expect("cleanup-fence downgrade worker");

    assert!(
        result.is_ok(),
        "cleanup-fence downgrade must inspect the schema after reserving its writer: {result:?}"
    );
    let connection = rusqlite::Connection::open(path).expect("post-race state");
    let cleanup_columns: (i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                WHERE name = 'recovery_cleanup_session_id'),
               (SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                WHERE name = 'concurrent_cleanup_session_id')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("count cleanup columns");
    assert_eq!(cleanup_columns, (0, 1));
}

#[test]
fn v10_downgrade_reserves_the_writer_before_reading_schema() {
    let temp = tempfile::TempDir::new().expect("temporary state directory");
    let path = temp.path().join("state.db");
    drop(Db::open_path(&path).expect("current receiver state"));
    super::super::schema::down_unavailable_notice_path(&path)
        .expect("stage adjacent v10 receiver state");

    let mut blocker = rusqlite::Connection::open(&path).expect("blocking connection");
    let blocker_transaction = blocker
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("reserve blocking writer");
    blocker_transaction
        .execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN concurrent_writer_marker TEXT;
             PRAGMA user_version = 9;
             INSERT OR REPLACE INTO meta (key, value)
               VALUES ('v10-down-race', 'held');",
        )
        .expect("hold concurrent v10 schema change");

    let (event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
    *SCHEMA_RACE_EVENTS
        .lock()
        .expect("install schema race sender") = Some(event_sender.clone());
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let result = super::super::schema::down_to_observation_path_with_busy_observer(
            &worker_path,
            report_schema_busy,
        )
        .map_err(|error| error.to_string());
        event_sender
            .send(SchemaRaceEvent::Finished(result))
            .expect("report v10 downgrade result");
    });

    assert!(matches!(
        event_receiver.recv().expect("first v10 downgrade event"),
        SchemaRaceEvent::Busy
    ));
    *SCHEMA_RACE_EVENTS
        .lock()
        .expect("clear schema race sender") = None;
    blocker_transaction
        .commit()
        .expect("release blocking writer");
    let result = loop {
        if let SchemaRaceEvent::Finished(result) = event_receiver
            .recv()
            .expect("v10 downgrade result after wait")
        {
            break result;
        }
    };
    worker.join().expect("v10 downgrade worker");

    assert!(
        result.is_ok(),
        "v10 downgrade must inspect the version after reserving its writer: {result:?}"
    );
    let connection = rusqlite::Connection::open(path).expect("post-race state");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version");
    let concurrent_marker: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name = 'concurrent_writer_marker'",
            [],
            |row| row.get(0),
        )
        .expect("count concurrent marker columns");
    assert_eq!(version, 9);
    assert_eq!(concurrent_marker, 1);
}
