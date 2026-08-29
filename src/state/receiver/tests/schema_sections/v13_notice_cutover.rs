#[test]
fn current_schema_moves_notice_gating_to_the_delivery_outbox() {
    let db = Db::open_in_memory().expect("receiver state");

    let version: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("receiver schema version");
    let obsolete_job_columns: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name IN (
               'pending_unavailable_notice',
               'unavailable_notice_owner',
               'unavailable_notice_expires_at_unix_ms'
             )",
            [],
            |row| row.get(0),
        )
        .expect("obsolete receiver-job notice columns");
    let delivery_contract: String = db
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_deliveries'",
            [],
            |row| row.get(0),
        )
        .expect("delivery table contract");

    assert_eq!(version, 13);
    assert_eq!(obsolete_job_columns, 0);
    assert!(delivery_contract.contains("'cleanup-gated'"));
}

#[test]
fn v12_upgrade_converts_pending_notices_to_cleanup_gated_or_ready_outbox_rows() {
    for (case, cleanup_instance, cleanup_session, expected_job, expected_delivery) in [
        ("cleanup-free", None, None, "answer-ready", "ready"),
        (
            "cleanup-fenced",
            Some("cleanup-instance"),
            Some("cleanup-session"),
            "failed",
            "cleanup-gated",
        ),
    ] {
        let temporary = tempfile::tempdir().expect("temporary receiver state");
        let path = temporary.path().join("state.db");
        let job_id = {
            let db = Db::open_path_with_legacy_identity(
                &path,
                &receiver_workspace_id().to_string(),
                receiver_user_id().as_str(),
            )
            .expect("v13 receiver state");
            let accepted = db
                .accept_receiver_job(
                    &receiver_job(Some(case), 100),
                    &ReceiverConversationIdentity::sms(
                        receiver_workspace_id(),
                        receiver_user_id(),
                    ),
                )
                .expect("accept receiver job");
            accepted.job_id().to_string()
        };
        super::super::schema::down_cutover_path(&path).expect("stage exact v12 schema");
        let connection = rusqlite::Connection::open(&path).expect("v12 receiver state");
        connection
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'failed', pending_unavailable_notice = 1,
                     last_error = 'recovery-attempt-exhausted',
                     recovery_cleanup_instance = ?2,
                     recovery_cleanup_session_id = ?3
                 WHERE job_id = ?1",
                rusqlite::params![job_id, cleanup_instance, cleanup_session],
            )
            .expect("stage pending v12 notice");

        super::super::schema::up(&connection, 12).expect("upgrade v12 notice state");

        let migrated: (String, String, i64) = connection
            .query_row(
                "SELECT job.state, delivery.state,
                        (SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                         WHERE name IN (
                           'pending_unavailable_notice',
                           'unavailable_notice_owner',
                           'unavailable_notice_expires_at_unix_ms'
                         ))
                 FROM receiver_jobs AS job
                 JOIN receiver_deliveries AS delivery
                   ON delivery.job_id = job.job_id
                  AND delivery.response_kind = 'unavailable-notice'
                 WHERE job.job_id = ?1",
                [&job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("load migrated unavailable notice");
        assert_eq!(migrated, (expected_job.to_owned(), expected_delivery.to_owned(), 0));
    }
}

#[test]
fn v13_down_reconstructs_the_v12_pending_notice_without_agent_replay() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("v13 receiver state");
        let inbound = receiver_job(Some("v13-down-cleanup-gated"), 100);
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let accepted = db
            .accept_receiver_job(&inbound, &identity)
            .expect("accept receiver job");
        let token = db
            .receiver_job(accepted.job_id())
            .expect("load accepted job")
            .expect("accepted job")
            .token();
        let notice = crate::server::reply::unanswered_notice("sms");
        assert!(
            super::super::store::response_intent::insert_with_state(
                &db.conn,
                accepted.job_id(),
                token,
                &inbound,
                ReceiverResponseKind::UnavailableNotice,
                &notice.text,
                ReceiverDeliveryState::CleanupGated,
                200,
            )
            .expect("insert cleanup-gated response")
        );
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'failed', last_error = 'recovery-attempt-exhausted',
                     recovery_cleanup_instance = 'cleanup-instance',
                     recovery_cleanup_session_id = 'cleanup-session'
                 WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("stage cleanup authority");
        accepted.job_id().to_string()
    };

    super::super::schema::down_cutover_path(&path).expect("downgrade v13 to v12");

    let connection = rusqlite::Connection::open(&path).expect("downgraded state");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("downgraded version");
    let source: (String, bool, Option<String>, Option<String>) = connection
        .query_row(
            "SELECT state, pending_unavailable_notice,
                    recovery_cleanup_instance, recovery_cleanup_session_id
             FROM receiver_jobs WHERE job_id = ?1",
            [&job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("downgraded source job");
    let gated_rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM receiver_deliveries
             WHERE job_id = ?1 AND state = 'cleanup-gated'",
            [&job_id],
            |row| row.get(0),
        )
        .expect("count unrepresentable gated rows");

    assert_eq!(version, 12);
    assert_eq!(
        source,
        (
            "failed".to_owned(),
            true,
            Some("cleanup-instance".to_owned()),
            Some("cleanup-session".to_owned()),
        )
    );
    assert_eq!(gated_rows, 0);
}

#[test]
fn v13_down_reserves_the_writer_before_inspecting_mutable_schema() {
    let _test_lock = SCHEMA_RACE_TEST_LOCK.lock().expect("schema race test lock");
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    drop(Db::open_path(&path).expect("v13 receiver state"));

    let mut blocker = rusqlite::Connection::open(&path).expect("blocking connection");
    let blocker_transaction = blocker
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("reserve blocking writer");
    blocker_transaction
        .execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN concurrent_v12_marker TEXT;
             INSERT OR REPLACE INTO meta (key, value) VALUES ('v13-down-race', 'held');",
        )
        .expect("hold concurrent schema writer");

    let (event_sender, event_receiver) = std::sync::mpsc::sync_channel(1);
    *SCHEMA_RACE_EVENTS.lock().expect("install race sender") = Some(event_sender.clone());
    let worker_path = path.clone();
    let worker = std::thread::spawn(move || {
        let result = super::super::schema::down_cutover_path_with_busy_observer(
            &worker_path,
            report_schema_busy,
        )
        .map_err(|error| error.to_string());
        event_sender
            .send(SchemaRaceEvent::Finished(result))
            .expect("report v13 downgrade result");
    });

    assert!(matches!(
        event_receiver.recv().expect("first v13 downgrade event"),
        SchemaRaceEvent::Busy
    ));
    *SCHEMA_RACE_EVENTS.lock().expect("clear race sender") = None;
    blocker_transaction.commit().expect("release writer");
    let result = loop {
        if let SchemaRaceEvent::Finished(result) =
            event_receiver.recv().expect("v13 result after wait")
        {
            break result;
        }
    };
    worker.join().expect("v13 downgrade worker");

    assert!(result.is_ok(), "v13 downgrade failed after writer wait");
    let connection = rusqlite::Connection::open(path).expect("downgraded state");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("downgraded version");
    let marker: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name = 'concurrent_v12_marker'",
            [],
            |row| row.get(0),
        )
        .expect("concurrent marker count");
    assert_eq!(version, 12);
    assert_eq!(marker, 0, "exact v12 rebuild retained a v13-only marker");
}
