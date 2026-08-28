const LOOSE_DELIVERY_TABLE_SQL: &str =
    "CREATE TABLE receiver_deliveries (
       delivery_id TEXT PRIMARY KEY,
       job_id TEXT NOT NULL,
       job_token TEXT NOT NULL,
       response_kind TEXT NOT NULL,
       envelope_json TEXT NOT NULL,
       state TEXT NOT NULL,
       attempt_id TEXT,
       attempt_count INTEGER NOT NULL DEFAULT 0,
       retry_at_unix_ms INTEGER,
       claim_owner TEXT,
       claim_expires_at_unix_ms INTEGER,
       first_attempt_at_unix_ms INTEGER,
       provider_reference TEXT,
       error_category TEXT,
       ambiguity_reason TEXT,
       created_at_unix_ms INTEGER NOT NULL,
       updated_at_unix_ms INTEGER NOT NULL
     );";

const LOOSE_DELIVERY_TABLE_WITHOUT_RETRY_SQL: &str =
    "CREATE TABLE receiver_deliveries (
       delivery_id TEXT PRIMARY KEY,
       job_id TEXT NOT NULL,
       job_token TEXT NOT NULL,
       response_kind TEXT NOT NULL,
       envelope_json TEXT NOT NULL,
       state TEXT NOT NULL,
       attempt_count INTEGER NOT NULL DEFAULT 0,
       created_at_unix_ms INTEGER NOT NULL,
       updated_at_unix_ms INTEGER NOT NULL
     );";

fn replace_delivery_table(connection: &rusqlite::Connection, schema: &str) {
    connection
        .execute_batch(
            "DROP INDEX IF EXISTS receiver_deliveries_due;
             DROP INDEX IF EXISTS receiver_deliveries_job_kind;
             DROP TABLE receiver_deliveries;",
        )
        .expect("drop current delivery table");
    connection
        .execute_batch(schema)
        .expect("create staged delivery table");
}

fn delivery_index_signature(
    connection: &rusqlite::Connection,
    index_name: &str,
) -> (bool, Vec<String>) {
    let unique = connection
        .query_row(
            "SELECT \"unique\" FROM pragma_index_list('receiver_deliveries')
             WHERE name = ?1",
            [index_name],
            |row| row.get::<_, bool>(0),
        )
        .expect("managed delivery index");
    let mut statement = connection
        .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
        .expect("delivery index columns");
    let columns = statement
        .query_map([index_name], |row| row.get::<_, String>(0))
        .expect("query delivery index columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect delivery index columns");
    (unique, columns)
}

#[test]
fn v12_same_version_repair_migrates_legacy_pending_notice_to_the_outbox() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("legacy-notice-repair"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept legacy notice job");
    db.conn
        .execute(
            "UPDATE receiver_jobs
             SET state = 'failed', pending_unavailable_notice = 1
             WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("stage legacy notice bit");

    super::super::schema::up(&db.conn, 12).expect("run same-version repair");

    let repaired: (String, bool, String) = db
        .conn
        .query_row(
            "SELECT job.state, job.pending_unavailable_notice, delivery.response_kind
             FROM receiver_jobs AS job
             JOIN receiver_deliveries AS delivery ON delivery.job_id = job.job_id
             WHERE job.job_id = ?1",
            [accepted.job_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load migrated legacy notice");
    assert_eq!(
        repaired,
        (
            "answer-ready".to_owned(),
            false,
            "unavailable-notice".to_owned()
        )
    );
}

#[test]
fn v12_repair_adds_retry_time_before_creating_the_due_index() {
    let db = Db::open_in_memory().expect("receiver state");
    replace_delivery_table(&db.conn, LOOSE_DELIVERY_TABLE_WITHOUT_RETRY_SQL);

    super::super::schema::up(&db.conn, 12)
        .expect("repair optional delivery columns before managed indexes");

    let retry_columns: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_deliveries')
             WHERE name = 'retry_at_unix_ms'",
            [],
            |row| row.get(0),
        )
        .expect("retry column count");
    assert_eq!(retry_columns, 1);
    assert_eq!(
        delivery_index_signature(&db.conn, "receiver_deliveries_due"),
        (
            false,
            vec![
                "state".to_owned(),
                "retry_at_unix_ms".to_owned(),
                "created_at_unix_ms".to_owned(),
                "delivery_id".to_owned(),
            ]
        )
    );
}

#[test]
fn v12_repair_adds_nullable_completion_evidence_without_losing_existing_deliveries() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("completion-evidence-repair"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");
    let job_id = accepted.job_id().to_string();
    let job_token = persisted_job_token(&db, accepted.job_id()).to_string();
    replace_delivery_table(&db.conn, LOOSE_DELIVERY_TABLE_SQL);
    db.conn
        .execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, created_at_unix_ms, updated_at_unix_ms)
             VALUES ('10000000-0000-4000-8000-000000000001', ?1, ?2,
                     'final-answer', '{}', 'ready', 0, 100, 100)",
            rusqlite::params![job_id, job_token],
        )
        .expect("stage pre-evidence delivery");

    super::super::schema::up(&db.conn, 12).expect("repair completion evidence column");

    let repaired: (i64, i64, Option<String>) = (
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('receiver_deliveries')
                 WHERE name = 'completion_evidence_json'",
                [],
                |row| row.get(0),
            )
            .expect("completion evidence column count"),
        db.conn
            .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| row.get(0))
            .expect("retained delivery count"),
        db.conn
            .query_row(
                "SELECT completion_evidence_json FROM receiver_deliveries",
                [],
                |row| row.get(0),
            )
            .expect("legacy completion evidence"),
    );
    assert_eq!(repaired, (1, 1, None));
}

#[test]
fn v12_repair_rebuilds_stale_managed_indexes_with_exact_signatures() {
    let db = Db::open_in_memory().expect("receiver state");
    db.conn
        .execute_batch(
            "DROP INDEX receiver_deliveries_job_kind;
             DROP INDEX receiver_deliveries_due;
             CREATE INDEX receiver_deliveries_job_kind
               ON receiver_deliveries(response_kind, job_id);
             CREATE UNIQUE INDEX receiver_deliveries_due
               ON receiver_deliveries(state, created_at_unix_ms);",
        )
        .expect("stage stale managed indexes");

    super::super::schema::up(&db.conn, 12).expect("repair managed delivery indexes");

    assert_eq!(
        delivery_index_signature(&db.conn, "receiver_deliveries_job_kind"),
        (
            true,
            vec!["job_id".to_owned(), "response_kind".to_owned()]
        )
    );
    assert_eq!(
        delivery_index_signature(&db.conn, "receiver_deliveries_due"),
        (
            false,
            vec![
                "state".to_owned(),
                "retry_at_unix_ms".to_owned(),
                "created_at_unix_ms".to_owned(),
                "delivery_id".to_owned(),
            ]
        )
    );
}

#[test]
fn v12_repair_fails_closed_before_rebuilding_uniqueness_over_duplicates() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("duplicate-delivery-repair"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");
    let job_id = accepted.job_id().to_string();
    let job_token = persisted_job_token(&db, accepted.job_id()).to_string();
    replace_delivery_table(&db.conn, LOOSE_DELIVERY_TABLE_SQL);
    db.conn
        .execute_batch(
            "CREATE INDEX receiver_deliveries_job_kind
               ON receiver_deliveries(response_kind, job_id);
             CREATE INDEX receiver_deliveries_due
               ON receiver_deliveries(state, created_at_unix_ms);",
        )
        .expect("stage stale managed indexes");
    for delivery_id in [
        "10000000-0000-4000-8000-000000000001",
        "20000000-0000-4000-8000-000000000002",
    ] {
        db.conn
            .execute(
                "INSERT INTO receiver_deliveries
                   (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                    attempt_count, created_at_unix_ms, updated_at_unix_ms)
                 VALUES (?1, ?2, ?3, 'final-answer', '{}', 'ready', 0, 100, 100)",
                rusqlite::params![delivery_id, job_id, job_token],
            )
            .expect("stage duplicate semantic response");
    }

    let error = super::super::schema::up(&db.conn, 12)
        .expect_err("duplicate semantic responses must fail schema repair closed");

    let rows: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| {
            row.get(0)
        })
        .expect("retained duplicate rows");
    assert_eq!(rows, 2);
    assert_eq!(
        delivery_index_signature(&db.conn, "receiver_deliveries_job_kind"),
        (
            false,
            vec!["response_kind".to_owned(), "job_id".to_owned()]
        )
    );
    assert!(!error.to_string().contains(&job_id));
    assert!(!error.to_string().contains(&job_token));
}

#[test]
fn v12_schema_rejects_blank_acknowledged_provider_references() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("blank-provider-reference"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");

    for provider_reference in ["", "   "] {
        let result = db.conn.execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, provider_reference, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, '{}', 'acknowledged', 1, ?5, 100, 100)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                accepted.job_id().to_string(),
                persisted_job_token(&db, accepted.job_id()).to_string(),
                if provider_reference.is_empty() {
                    "final-answer"
                } else {
                    "fallback-notice"
                },
                provider_reference,
            ],
        );
        assert!(result.is_err(), "blank provider reference was acknowledged");
    }
}

#[test]
fn v12_repair_turns_blank_acknowledgement_into_explicit_ambiguity() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("repair-blank-provider-reference"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");
    let job_id = accepted.job_id().to_string();
    let job_token = persisted_job_token(&db, accepted.job_id()).to_string();
    replace_delivery_table(&db.conn, LOOSE_DELIVERY_TABLE_SQL);
    db.conn
        .execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, provider_reference, created_at_unix_ms, updated_at_unix_ms)
             VALUES ('10000000-0000-4000-8000-000000000001', ?1, ?2,
                     'final-answer', '{}', 'acknowledged', 1, '   ', 100, 100)",
            rusqlite::params![job_id, job_token],
        )
        .expect("stage legacy blank acknowledgement");

    super::super::schema::up(&db.conn, 12).expect("repair blank acknowledgement");

    let repaired: (String, Option<String>, Option<String>) = db
        .conn
        .query_row(
            "SELECT state, provider_reference, ambiguity_reason FROM receiver_deliveries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("repaired acknowledgement");
    let table_sql: String = db
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_deliveries'",
            [],
            |row| row.get(0),
        )
        .expect("repaired table contract");
    assert_eq!(
        repaired,
        (
            "ambiguous".to_owned(),
            None,
            Some("provider-acknowledgement-malformed".to_owned())
        )
    );
    assert!(table_sql.contains("length(trim(provider_reference)) > 0"));
}

#[test]
fn v12_down_never_maps_a_blank_acknowledgement_to_done() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let accepted = db
            .accept_receiver_job(
                &receiver_job(Some("down-blank-provider-reference"), 100),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        let job_id = accepted.job_id().to_string();
        let job_token = persisted_job_token(&db, accepted.job_id()).to_string();
        db.conn
            .execute(
                "UPDATE receiver_jobs SET state = 'answer-ready' WHERE job_id = ?1",
                [&job_id],
            )
            .expect("stage answer-ready job");
        replace_delivery_table(&db.conn, LOOSE_DELIVERY_TABLE_SQL);
        db.conn
            .execute(
                "INSERT INTO receiver_deliveries
                   (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                    attempt_count, provider_reference, created_at_unix_ms, updated_at_unix_ms)
                 VALUES ('10000000-0000-4000-8000-000000000001', ?1, ?2,
                         'final-answer', '{}', 'acknowledged', 1, '   ', 100, 100)",
                rusqlite::params![job_id, job_token],
            )
            .expect("stage blank acknowledged delivery");
        job_id
    };

    super::super::schema::down_delivery_path(&path)
        .expect("downgrade legacy blank acknowledgement safely");

    let connection = rusqlite::Connection::open(path).expect("downgraded state");
    let state: (String, Option<String>) = connection
        .query_row(
            "SELECT state, last_error FROM receiver_jobs WHERE job_id = ?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("downgraded job state");
    assert_eq!(
        state,
        ("failed".to_owned(), Some("downgrade-no-replay".to_owned()))
    );
}

#[test]
fn v12_same_version_repair_terminalizes_completed_retry_without_delivery_row() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let accepted = db
            .accept_receiver_job(
                &receiver_job(Some("missing-final-answer-row"), 100),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'retrying', completed_at_unix_ms = 100,
                     retry_at_unix_ms = NULL, retry_from_state = NULL
                 WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("stage completed delivery retry without outbox row");
        accepted.job_id()
    };

    let repaired = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("same-version v12 repair");
    let state: (String, Option<String>) = repaired
        .conn
        .query_row(
            "SELECT state, last_error FROM receiver_jobs WHERE job_id = ?1",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load repaired job");

    assert!(
        state
            == (
                "failed".to_owned(),
                Some("delivery-schema-repair-missing".to_owned())
            ),
        "completed delivery retry without its outbox row remained unclaimable"
    );
}

#[test]
fn v12_down_up_terminalizes_completed_retry_when_delivery_table_is_missing() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let job_id = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let accepted = db
            .accept_receiver_job(
                &receiver_job(Some("missing-final-answer-table"), 100),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'retrying', completed_at_unix_ms = 100,
                     retry_at_unix_ms = NULL, retry_from_state = NULL
                 WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("stage completed delivery retry");
        db.conn
            .execute_batch(
                "DROP INDEX IF EXISTS receiver_deliveries_due;
                 DROP INDEX IF EXISTS receiver_deliveries_job_kind;
                 DROP TABLE receiver_deliveries;",
            )
            .expect("remove delivery table before downgrade");
        accepted.job_id()
    };

    super::super::schema::down_delivery_path(&path)
        .expect("downgrade v12 state without delivery table");
    let downgraded = rusqlite::Connection::open(&path).expect("downgraded state");
    let version: i64 = downgraded
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("v11 schema version");
    let v11_state: (String, Option<String>) = downgraded
        .query_row(
            "SELECT state, last_error FROM receiver_jobs WHERE job_id = ?1",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load downgraded job");
    assert!(
        version == 11
            && v11_state
                == (
                    "failed".to_owned(),
                    Some("downgrade-no-replay".to_owned())
                ),
        "downgrade retained an unclaimable completed delivery retry"
    );
    drop(downgraded);

    let upgraded = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("upgrade repaired v11 state");
    let v12_state: (String, i64) = upgraded
        .conn
        .query_row(
            "SELECT job.state,
                    (SELECT COUNT(*) FROM receiver_deliveries AS delivery
                     WHERE delivery.job_id = job.job_id)
             FROM receiver_jobs AS job WHERE job.job_id = ?1",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load upgraded terminal job");
    assert!(
        v12_state == ("failed".to_owned(), 0),
        "up migration recreated replay authority for the terminal job"
    );
}
