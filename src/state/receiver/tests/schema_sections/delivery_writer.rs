#[test]
fn v11_upgrade_reserves_the_writer_before_inspecting_delivery_schema() {
    let _test_lock = SCHEMA_RACE_TEST_LOCK.lock().expect("schema race test lock");
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    drop(Db::open_path(&path).expect("current receiver state"));
    super::super::schema::down_delivery_path(&path).expect("stage adjacent v11 state");

    let mut blocker = rusqlite::Connection::open(&path).expect("blocking connection");
    let blocker_transaction = blocker
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("reserve blocking writer");
    blocker_transaction
        .execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN concurrent_v12_marker TEXT;
             INSERT OR REPLACE INTO meta (key, value) VALUES ('v12-up-race', 'held');",
        )
        .expect("hold concurrent schema writer");

    let candidate = rusqlite::Connection::open(&path).expect("candidate connection");
    candidate
        .busy_handler(Some(report_schema_busy))
        .expect("observe candidate writer wait");
    let (event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
    *SCHEMA_RACE_EVENTS.lock().expect("install race sender") = Some(event_sender.clone());
    let worker = std::thread::spawn(move || {
        let result = super::super::schema::up(&candidate, 11).map_err(|error| error.to_string());
        event_sender
            .send(SchemaRaceEvent::Finished(result))
            .expect("report v12 upgrade result");
    });

    assert!(matches!(
        event_receiver.recv().expect("first v12 upgrade event"),
        SchemaRaceEvent::Busy
    ));
    *SCHEMA_RACE_EVENTS.lock().expect("clear race sender") = None;
    blocker_transaction.commit().expect("release writer");
    let result = loop {
        if let SchemaRaceEvent::Finished(result) =
            event_receiver.recv().expect("v12 result after wait")
        {
            break result;
        }
    };
    worker.join().expect("v12 upgrade worker");

    assert!(result.is_ok(), "v12 upgrade failed after writer wait");
    let connection = rusqlite::Connection::open(path).expect("upgraded state");
    let marker: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name = 'concurrent_v12_marker'",
            [],
            |row| row.get(0),
        )
        .expect("concurrent marker count");
    assert_eq!(marker, 1);
}

#[test]
fn v12_downgrade_reserves_the_writer_before_inspecting_v11_shape() {
    let _test_lock = SCHEMA_RACE_TEST_LOCK.lock().expect("schema race test lock");
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    drop(Db::open_path(&path).expect("current receiver state"));

    let mut blocker = rusqlite::Connection::open(&path).expect("blocking connection");
    let blocker_transaction = blocker
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("reserve blocking writer");
    blocker_transaction
        .execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN concurrent_v11_marker TEXT;
             INSERT OR REPLACE INTO meta (key, value) VALUES ('v12-down-race', 'held');",
        )
        .expect("hold concurrent schema writer");

    let (event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
    *SCHEMA_RACE_EVENTS.lock().expect("install race sender") = Some(event_sender.clone());
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let result = super::super::schema::down_delivery_path_with_busy_observer(
            &worker_path,
            report_schema_busy,
        )
        .map_err(|error| error.to_string());
        event_sender
            .send(SchemaRaceEvent::Finished(result))
            .expect("report v12 downgrade result");
    });

    assert!(matches!(
        event_receiver.recv().expect("first v12 downgrade event"),
        SchemaRaceEvent::Busy
    ));
    *SCHEMA_RACE_EVENTS.lock().expect("clear race sender") = None;
    blocker_transaction.commit().expect("release writer");
    let result = loop {
        if let SchemaRaceEvent::Finished(result) =
            event_receiver.recv().expect("v12 result after wait")
        {
            break result;
        }
    };
    worker.join().expect("v12 downgrade worker");

    assert!(result.is_ok(), "v12 downgrade failed after writer wait");
    let connection = rusqlite::Connection::open(path).expect("downgraded state");
    let marker: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name = 'concurrent_v11_marker'",
            [],
            |row| row.get(0),
        )
        .expect("concurrent marker count");
    assert!(
        marker == 0,
        "exact v11 rebuild retained the concurrent v12-only marker"
    );
}
