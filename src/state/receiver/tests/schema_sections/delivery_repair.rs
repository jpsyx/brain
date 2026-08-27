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
