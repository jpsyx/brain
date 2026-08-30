#[test]
fn v12_upgrade_converts_pending_notice_from_every_valid_job_state() {
    for state in [
        "queued",
        "claimed",
        "launching",
        "launched",
        "accepted",
        "processing",
        "answer-ready",
        "delivering",
        "retrying",
        "failed",
        "done",
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
            db.accept_receiver_job(
                &receiver_job(Some(state), 100),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job")
            .job_id()
            .to_string()
        };
        super::super::schema::down_cutover_path(&path).expect("stage exact v12 schema");
        let connection = rusqlite::Connection::open(&path).expect("v12 receiver state");
        connection
            .execute(
                "UPDATE receiver_jobs
                 SET state = ?2, pending_unavailable_notice = 1,
                     last_error = 'recovery-attempt-exhausted',
                     claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                     retry_at_unix_ms = NULL, retry_from_state = NULL
                 WHERE job_id = ?1",
                rusqlite::params![job_id, state],
            )
            .expect("stage valid v12 pending state");

        super::super::schema::up(&connection, 12).expect("upgrade every pending v12 state");

        let migrated: (String, String, i64) = connection
            .query_row(
                "SELECT job.state, delivery.state, COUNT(*) OVER ()
                 FROM receiver_jobs AS job
                 JOIN receiver_deliveries AS delivery
                   ON delivery.job_id = job.job_id
                  AND delivery.job_token = job.job_token
                  AND delivery.response_kind = 'unavailable-notice'
                 WHERE job.job_id = ?1",
                [&job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("load migrated pending state");
        assert_eq!(
            migrated,
            ("answer-ready".to_owned(), "ready".to_owned(), 1),
            "valid v12 state {state} lost its pending semantic response"
        );
    }
}

#[test]
fn partial_v13_down_up_recreates_exact_delivery_contract_idempotently() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let db = Db::open_path(&path).expect("v13 receiver state");
    db.conn
        .execute_batch("DROP TABLE receiver_deliveries;")
        .expect("stage partial v13 state");
    drop(db);

    super::super::schema::down_cutover_path(&path).expect("first v13 downgrade");
    super::super::schema::down_cutover_path(&path).expect("idempotent v12 downgrade");

    let connection = rusqlite::Connection::open(&path).expect("v12 receiver state");
    let v12_contract = stored_receiver_table_sql(&connection, "receiver_deliveries");
    let v12_indexes: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index'
               AND name IN ('receiver_deliveries_due', 'receiver_deliveries_job_kind')",
            [],
            |row| row.get(0),
        )
        .expect("v12 managed delivery indexes");
    assert!(!v12_contract.contains("'cleanup-gated'"));
    assert_eq!(v12_indexes, 2);

    super::super::schema::up(&connection, 12).expect("first v13 re-upgrade");
    super::super::schema::up(&connection, 13).expect("idempotent v13 re-upgrade");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("re-upgraded version");
    let v13_contract = stored_receiver_table_sql(&connection, "receiver_deliveries");
    assert_eq!(version, 13);
    assert!(v13_contract.contains("'cleanup-gated'"));
    drop(connection);

    super::super::schema::down_cutover_path(&path).expect("second v13 downgrade");
    let connection = rusqlite::Connection::open(path).expect("second v12 state");
    assert_eq!(
        stored_receiver_table_sql(&connection, "receiver_deliveries"),
        v12_contract
    );
}

#[test]
fn v12_upgrade_rejects_a_conflicting_terminal_semantic_row_without_losing_pending_authority() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("v13 receiver state");
        let inbound = receiver_job(Some("conflicting-terminal-row"), 100);
        let accepted = db
            .accept_receiver_job(
                &inbound,
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        let token = db
            .receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .token();
        assert!(
            super::super::store::response_intent::insert(
                &db.conn,
                accepted.job_id(),
                token,
                &inbound,
                ReceiverResponseKind::UnavailableNotice,
                "unavailable",
                200,
            )
            .expect("insert semantic row")
        );
        accepted.job_id().to_string()
    };
    super::super::schema::down_cutover_path(&path).expect("stage exact v12 schema");
    let connection = rusqlite::Connection::open(&path).expect("v12 receiver state");
    connection
        .execute_batch(
            "UPDATE receiver_deliveries
             SET state = 'failed', error_category = 'invalid-request',
                 fallback_decision = 'no-safe-fallback';",
        )
        .expect("stage conflicting terminal delivery");
    connection
        .execute(
            "UPDATE receiver_jobs
             SET state = 'queued', pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [&job_id],
        )
        .expect("stage pending v12 authority");

    let error = super::super::schema::up(&connection, 12)
        .expect_err("conflicting semantic state must abort the cutover");

    let preserved: (i64, String, bool, String, i64) = connection
        .query_row(
            "SELECT
               (SELECT user_version FROM pragma_user_version),
               job.state, job.pending_unavailable_notice, delivery.state,
               (SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                WHERE name = 'unavailable_notice_owner')
             FROM receiver_jobs AS job
             JOIN receiver_deliveries AS delivery ON delivery.job_id = job.job_id
             WHERE job.job_id = ?1",
            [&job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("load rolled-back v12 authority");
    assert!(error.to_string().contains("conflicting unavailable-notice"));
    assert_eq!(
        preserved,
        (12, "queued".to_owned(), true, "failed".to_owned(), 1)
    );
}

#[test]
fn v12_upgrade_rejects_a_different_valid_notice_envelope_without_losing_pending_authority() {
    let temporary = tempfile::tempdir().expect("temporary receiver state");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("v13 receiver state");
        let inbound = receiver_job(Some("conflicting-valid-envelope"), 100);
        let accepted = db
            .accept_receiver_job(
                &inbound,
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        let token = db
            .receiver_job(accepted.job_id())
            .expect("load receiver job")
            .expect("receiver job")
            .token();
        assert!(
            super::super::store::response_intent::insert(
                &db.conn,
                accepted.job_id(),
                token,
                &inbound,
                ReceiverResponseKind::UnavailableNotice,
                "different valid notice body",
                200,
            )
            .expect("insert semantic row")
        );
        db.conn
            .execute(
                "UPDATE receiver_deliveries
                 SET envelope_json = ?1
                 WHERE job_id = ?2 AND response_kind = 'unavailable-notice'",
                rusqlite::params![
                    serde_json::json!({
                        "channel": "sms",
                        "value": {
                            "sender": "+12125550101",
                            "recipient": "+12125550102",
                            "body": "different valid notice body",
                            "long_form_available": false
                        }
                    })
                    .to_string(),
                    accepted.job_id().to_string(),
                ],
            )
            .expect("replace with another valid envelope");
        accepted.job_id().to_string()
    };
    super::super::schema::down_cutover_path(&path).expect("stage exact v12 schema");
    let connection = rusqlite::Connection::open(&path).expect("v12 receiver state");
    connection
        .execute(
            "UPDATE receiver_jobs
             SET state = 'queued', pending_unavailable_notice = 1,
                 last_error = 'recovery-attempt-exhausted'
             WHERE job_id = ?1",
            [&job_id],
        )
        .expect("stage pending v12 authority");

    let error = super::super::schema::up(&connection, 12)
        .expect_err("different valid envelope must abort the cutover");

    let preserved: (i64, String, bool, String, i64) = connection
        .query_row(
            "SELECT
               (SELECT user_version FROM pragma_user_version),
               job.state, job.pending_unavailable_notice, delivery.state,
               (SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                WHERE name = 'unavailable_notice_owner')
             FROM receiver_jobs AS job
             JOIN receiver_deliveries AS delivery ON delivery.job_id = job.job_id
             WHERE job.job_id = ?1",
            [&job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("load rolled-back v12 authority");
    assert!(error.to_string().contains("conflicting unavailable-notice"));
    assert_eq!(
        preserved,
        (12, "queued".to_owned(), true, "ready".to_owned(), 1)
    );
}

#[test]
fn declined_notice_storage_write_rolls_back_the_entire_v12_cutover() {
    let db = Db::open_in_memory().expect("v13 receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("declined-storage-write"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");
    db.conn
        .execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN pending_unavailable_notice
               INTEGER NOT NULL DEFAULT 0 CHECK (pending_unavailable_notice IN (0, 1));
             ALTER TABLE receiver_jobs ADD COLUMN unavailable_notice_owner TEXT;
             ALTER TABLE receiver_jobs ADD COLUMN unavailable_notice_expires_at_unix_ms INTEGER;
             CREATE TRIGGER decline_unavailable_notice_insert
             BEFORE INSERT ON receiver_deliveries
             WHEN NEW.response_kind = 'unavailable-notice'
             BEGIN
               SELECT RAISE(IGNORE);
             END;
             PRAGMA user_version = 12;",
        )
        .expect("stage partial v12 storage failure");
    db.conn
        .execute(
            "UPDATE receiver_jobs SET pending_unavailable_notice = 1 WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("stage pending authority");

    super::super::schema::up(&db.conn, 12)
        .expect_err("declined semantic insert must roll back the cutover");

    let preserved: (i64, bool, i64, i64) = db
        .conn
        .query_row(
            "SELECT
               (SELECT user_version FROM pragma_user_version),
               pending_unavailable_notice,
               (SELECT COUNT(*) FROM receiver_deliveries WHERE job_id = ?1),
               (SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                WHERE name IN (
                  'pending_unavailable_notice', 'unavailable_notice_owner',
                  'unavailable_notice_expires_at_unix_ms'
                ))
             FROM receiver_jobs WHERE job_id = ?1",
            [accepted.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load rolled-back storage authority");
    assert_eq!(preserved, (12, true, 0, 3));
}
