fn seed_delivery_row(
    db: &Db,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    delivery_id: &str,
    response_kind: &str,
    state: &str,
    envelope: &str,
) {
    db.conn
        .execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json, state,
                attempt_count, provider_reference, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, 100, 100)",
            rusqlite::params![
                delivery_id,
                job_id.to_string(),
                token.to_string(),
                response_kind,
                envelope,
                state,
                (state == "acknowledged").then_some("provider-acknowledgement"),
            ],
        )
        .expect("seed receiver delivery");
}

fn persisted_job_token(db: &Db, job_id: ReceiverJobId) -> ReceiverJobToken {
    let token: String = db
        .conn
        .query_row(
            "SELECT job_token FROM receiver_jobs WHERE job_id = ?1",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("persisted receiver job token");
    ReceiverJobToken::parse(&token).expect("valid persisted job token")
}

#[test]
fn v12_schema_creates_the_content_outbox_without_credential_columns() {
    let db = Db::open_in_memory().expect("receiver state");

    let version: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("receiver schema version");
    let sql: String = db
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_deliveries'",
            [],
            |row| row.get(0),
        )
        .expect("delivery outbox schema");
    let cleanup_sql: String = db
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_answer_cleanups'",
            [],
            |row| row.get(0),
        )
        .expect("answer cleanup schema");

    assert_eq!(version, 12);
    for column in [
        "delivery_id",
        "job_id",
        "job_token",
        "response_kind",
        "envelope_json",
        "completion_evidence_json",
        "state",
        "attempt_id",
        "attempt_count",
        "retry_at_unix_ms",
        "claim_owner",
        "claim_expires_at_unix_ms",
        "first_attempt_at_unix_ms",
        "provider_reference",
        "error_category",
        "ambiguity_reason",
        "created_at_unix_ms",
        "updated_at_unix_ms",
    ] {
        assert!(sql.contains(column), "missing delivery column {column}");
    }
    for forbidden in ["api_key", "auth_token", "password", "secret"] {
        assert!(!sql.to_ascii_lowercase().contains(forbidden));
        assert!(!cleanup_sql.to_ascii_lowercase().contains(forbidden));
    }
    for column in [
        "job_id",
        "job_token",
        "workspace_id",
        "conversation_id",
        "brain_instance_id",
        "agent_kind",
        "actor_id",
        "channel",
        "registered_session_id",
        "actual_session_id",
        "controller_shutdown_acknowledged",
        "session_released",
        "artifacts_removed",
        "created_at_unix_ms",
        "updated_at_unix_ms",
    ] {
        assert!(cleanup_sql.contains(column), "missing cleanup column {column}");
    }
}

#[test]
fn v12_repair_recreates_a_missing_answer_cleanup_table() {
    let db = Db::open_in_memory().expect("receiver state");
    db.conn
        .execute_batch("DROP TABLE receiver_answer_cleanups;")
        .expect("stage missing answer cleanup table");

    super::super::schema::up(&db.conn, 12).expect("repair answer cleanup table");

    let tables: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_answer_cleanups'",
            [],
            |row| row.get(0),
        )
        .expect("answer cleanup table count");
    assert_eq!(tables, 1);
}

#[test]
fn v12_repair_adds_the_controller_shutdown_acknowledgement_to_an_older_cleanup_table() {
    let db = Db::open_in_memory().expect("receiver state");
    db.conn
        .execute_batch(
            "ALTER TABLE receiver_answer_cleanups
             DROP COLUMN controller_shutdown_acknowledged;",
        )
        .expect("stage pre-handoff answer cleanup table");

    super::super::schema::up(&db.conn, 12).expect("repair answer cleanup handoff column");

    let column: (String, String) = db
        .conn
        .query_row(
            "SELECT name, dflt_value FROM pragma_table_info('receiver_answer_cleanups')
             WHERE name = 'controller_shutdown_acknowledged'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("controller shutdown acknowledgement column");
    assert_eq!(column, ("controller_shutdown_acknowledged".to_owned(), "0".to_owned()));
}

#[test]
fn v12_repair_removes_the_legacy_cleanup_instance_unique_index() {
    let db = Db::open_in_memory().expect("receiver state");
    db.conn
        .execute_batch(
            "CREATE UNIQUE INDEX legacy_cleanup_instance_unique
               ON receiver_answer_cleanups(workspace_id, brain_instance_id);",
        )
        .expect("stage an explicitly named legacy cleanup uniqueness");

    super::super::schema::up(&db.conn, 12).expect("repair cleanup uniqueness");

    let unique_instance_indexes: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_index_list('receiver_answer_cleanups') AS indexes
             WHERE indexes.\"unique\" = 1
               AND (
                 SELECT group_concat(name, ',')
                 FROM (
                   SELECT name FROM pragma_index_info(indexes.name) ORDER BY seqno
                 )
               ) = 'workspace_id,brain_instance_id'",
            [],
            |row| row.get(0),
        )
        .expect("legacy cleanup uniqueness count");
    assert_eq!(unique_instance_indexes, 0);
}

#[test]
fn one_job_can_have_one_row_per_semantic_response_kind() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("semantic-kind"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");
    seed_delivery_row(
        &db,
        accepted.job_id(),
        persisted_job_token(&db, accepted.job_id()),
        "10000000-0000-4000-8000-000000000001",
        "final-answer",
        "ready",
        r#"{"channel":"sms","value":{"recipient":"private","body":"answer","long_form_available":false}}"#,
    );
    seed_delivery_row(
        &db,
        accepted.job_id(),
        persisted_job_token(&db, accepted.job_id()),
        "20000000-0000-4000-8000-000000000002",
        "fallback-notice",
        "ready",
        r#"{"channel":"sms","value":{"recipient":"private","body":"fallback","long_form_available":false}}"#,
    );

    let duplicate = db.conn.execute(
        "INSERT INTO receiver_deliveries
           (delivery_id, job_id, job_token, response_kind, envelope_json, state,
            attempt_count, created_at_unix_ms, updated_at_unix_ms)
         VALUES ('30000000-0000-4000-8000-000000000003', ?1, ?2,
                 'final-answer', '{}', 'ready', 0, 100, 100)",
        rusqlite::params![
            accepted.job_id().to_string(),
            persisted_job_token(&db, accepted.job_id()).to_string()
        ],
    );

    assert!(duplicate.is_err(), "duplicate semantic response was accepted");
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| row.get(0))
        .expect("delivery count");
    assert_eq!(count, 2);
}

#[test]
fn v12_repair_terminalizes_an_interrupted_partial_delivery_lease() {
    let db = Db::open_in_memory().expect("receiver state");
    let accepted = db
        .accept_receiver_job(
            &receiver_job(Some("partial-delivery-lease"), 100),
            &ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id()),
        )
        .expect("accept receiver job");
    seed_delivery_row(
        &db,
        accepted.job_id(),
        persisted_job_token(&db, accepted.job_id()),
        "10000000-0000-4000-8000-000000000001",
        "final-answer",
        "ready",
        r#"{"channel":"sms","value":{"recipient":"private","body":"private-answer","long_form_available":false}}"#,
    );
    db.conn
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE receiver_deliveries
             SET state = 'delivering', attempt_id = '20000000-0000-4000-8000-000000000002',
                 attempt_count = 1, claim_owner = 'interrupted-owner',
                 claim_expires_at_unix_ms = NULL, first_attempt_at_unix_ms = 100;
             PRAGMA ignore_check_constraints = OFF;",
        )
        .expect("stage interrupted delivery lease");

    super::super::schema::up(&db.conn, 12).expect("repair v12 delivery schema");

    let repaired: (String, Option<String>, Option<i64>, Option<String>) = db
        .conn
        .query_row(
            "SELECT state, claim_owner, claim_expires_at_unix_ms, ambiguity_reason
             FROM receiver_deliveries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("load repaired delivery");
    assert_eq!(
        repaired,
        (
            "ambiguous".to_owned(),
            None,
            None,
            Some("result-commit-unknown".to_owned())
        )
    );
}

#[test]
fn v12_down_preserves_transcripts_and_maps_acknowledged_and_unacknowledged_jobs() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let (acknowledged_id, unacknowledged_id, conversation_id) = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        let acknowledged = db
            .accept_receiver_job(&receiver_job(Some("acknowledged-down"), 100), &identity)
            .expect("accept acknowledged job");
        let unacknowledged = db
            .accept_receiver_job(&receiver_job(Some("ready-down"), 200), &identity)
            .expect("accept unacknowledged job");
        db.update_receiver_conversation(
            acknowledged.conversation_id(),
            "private portable transcript",
            None,
            300,
        )
        .expect("store portable transcript");
        seed_delivery_row(
            &db,
            acknowledged.job_id(),
            persisted_job_token(&db, acknowledged.job_id()),
            "10000000-0000-4000-8000-000000000001",
            "final-answer",
            "acknowledged",
            r#"{"channel":"sms","value":{"recipient":"private","body":"private-ack","long_form_available":false}}"#,
        );
        seed_delivery_row(
            &db,
            unacknowledged.job_id(),
            persisted_job_token(&db, unacknowledged.job_id()),
            "20000000-0000-4000-8000-000000000002",
            "final-answer",
            "ready",
            r#"{"channel":"sms","value":{"recipient":"private","body":"private-ready","long_form_available":false}}"#,
        );
        (
            acknowledged.job_id().to_string(),
            unacknowledged.job_id().to_string(),
            acknowledged.conversation_id().to_string(),
        )
    };

    super::super::schema::down_delivery_path(&path).expect("downgrade delivery outbox");

    let connection = rusqlite::Connection::open(path).expect("downgraded state");
    let states: (String, String, Option<String>) = connection
        .query_row(
            "SELECT
               (SELECT state FROM receiver_jobs WHERE job_id = ?1),
               (SELECT state FROM receiver_jobs WHERE job_id = ?2),
               (SELECT last_error FROM receiver_jobs WHERE job_id = ?2)",
            rusqlite::params![acknowledged_id, unacknowledged_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load downgraded jobs");
    let transcript: String = connection
        .query_row(
            "SELECT transcript_markdown FROM receiver_conversations WHERE conversation_id = ?1",
            [&conversation_id],
            |row| row.get(0),
        )
        .expect("load retained transcript");
    let delivery_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_deliveries'",
            [],
            |row| row.get(0),
        )
        .expect("delivery table count");
    let cleanup_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_answer_cleanups'",
            [],
            |row| row.get(0),
        )
        .expect("answer cleanup table count");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("downgraded version");

    assert_eq!(states.0, "done");
    assert_eq!(states.1, "failed");
    assert_eq!(states.2.as_deref(), Some("downgrade-no-replay"));
    assert!(!states.2.unwrap_or_default().contains("private"));
    assert_eq!(transcript, "private portable transcript");
    assert_eq!(delivery_tables, 0);
    assert_eq!(cleanup_tables, 0);
    assert_eq!(version, 11);
}

#[test]
fn v12_down_keeps_the_outbox_when_the_v11_shape_is_not_valid() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    {
        let db = Db::open_path(&path).expect("receiver state");
        db.conn
            .execute_batch(
                "ALTER TABLE receiver_jobs DROP COLUMN unavailable_notice_owner;",
            )
            .expect("damage v11 receiver shape");
    }

    let error = super::super::schema::down_delivery_path(&path)
        .expect_err("invalid v11 shape must block downgrade");

    let connection = rusqlite::Connection::open(path).expect("unchanged v12 state");
    let retained: (i64, i64) = (
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("retained version"),
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'receiver_deliveries'",
                [],
                |row| row.get(0),
            )
            .expect("retained outbox count"),
    );
    assert_eq!(retained, (12, 1));
    assert!(!error.to_string().contains("private"));
}
