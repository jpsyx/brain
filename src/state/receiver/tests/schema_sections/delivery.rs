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

const PRE_TASK3_0473434_DELIVERY_TABLE: &str = "CREATE TABLE receiver_deliveries (
           delivery_id                 TEXT PRIMARY KEY,
           job_id                      TEXT NOT NULL REFERENCES receiver_jobs(job_id) ON DELETE CASCADE,
           job_token                   TEXT NOT NULL,
           response_kind               TEXT NOT NULL CHECK (response_kind IN (
             'final-answer', 'unavailable-notice', 'control-acknowledgement', 'fallback-notice'
           )),
           envelope_json               TEXT NOT NULL,
           completion_evidence_json    TEXT,
           state                       TEXT NOT NULL CHECK (state IN (
             'ready', 'delivering', 'retrying', 'acknowledged', 'failed', 'ambiguous'
           )),
           attempt_id                  TEXT,
           attempt_count               INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
           retry_at_unix_ms             INTEGER,
           claim_owner                 TEXT,
           claim_expires_at_unix_ms    INTEGER,
           first_attempt_at_unix_ms    INTEGER,
           provider_reference          TEXT,
           error_category              TEXT CHECK (error_category IN (
             'authorization', 'credentials', 'invalid-request', 'provider-rejected',
             'transport-unavailable', 'retry-exhausted', 'idempotency-window-expired'
           )),
           ambiguity_reason            TEXT CHECK (ambiguity_reason IN (
             'provider-acceptance-unknown', 'provider-acknowledgement-malformed',
             'result-commit-unknown', 'idempotency-window-expired'
           )),
           created_at_unix_ms          INTEGER NOT NULL,
           updated_at_unix_ms          INTEGER NOT NULL,
           UNIQUE (job_id, response_kind),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL)),
           CHECK (state = 'delivering' OR claim_owner IS NULL),
           CHECK (state != 'delivering' OR (
             attempt_id IS NOT NULL AND claim_owner IS NOT NULL
             AND first_attempt_at_unix_ms IS NOT NULL
           )),
           CHECK (state = 'retrying' OR retry_at_unix_ms IS NULL),
           CHECK (state != 'retrying' OR retry_at_unix_ms IS NOT NULL),
           CHECK (state != 'acknowledged' OR (
             provider_reference IS NOT NULL AND length(trim(provider_reference)) > 0
           )),
           CHECK (state != 'ambiguous' OR ambiguity_reason IS NOT NULL)
         );";

#[test]
fn v12_repair_rebuilds_the_real_0473434_delivery_shape_before_claiming() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact answer owner");
    let frozen_before: String = fixture
        .db
        .conn
        .query_row("SELECT envelope_json FROM receiver_deliveries", [], |row| row.get(0))
        .expect("frozen delivery before compatibility repair");
    fixture
        .db
        .conn
        .execute_batch(
            "DROP INDEX receiver_deliveries_due;
             ALTER TABLE receiver_deliveries RENAME TO receiver_deliveries_task3_current;",
        )
        .expect("stage the pre-Task3 table boundary");
    fixture
        .db
        .conn
        .execute_batch(PRE_TASK3_0473434_DELIVERY_TABLE)
        .expect("create exact 0473434 delivery table");
    fixture
        .db
        .conn
        .execute_batch(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json,
                completion_evidence_json, state, attempt_id, attempt_count,
                retry_at_unix_ms, claim_owner, claim_expires_at_unix_ms,
                first_attempt_at_unix_ms, provider_reference, error_category,
                ambiguity_reason, created_at_unix_ms, updated_at_unix_ms)
             SELECT delivery_id, job_id, job_token, response_kind, envelope_json,
                    completion_evidence_json, state, attempt_id, attempt_count,
                    retry_at_unix_ms, claim_owner, claim_expires_at_unix_ms,
                    first_attempt_at_unix_ms, provider_reference, error_category,
                    ambiguity_reason, created_at_unix_ms, updated_at_unix_ms
             FROM receiver_deliveries_task3_current;
             DROP TABLE receiver_deliveries_task3_current;",
        )
        .expect("preserve the baseline delivery row");

    super::super::schema::up(&fixture.db.conn, 12).expect("repair same-version v12 shape");

    let claim = fixture
        .db
        .claim_next_receiver_delivery("compatibility-owner", 2_000, 32_000)
        .expect("claim after same-version repair")
        .expect("preserved answer remains due");
    assert_eq!(claim.job_id(), fixture.job_id);
    assert!(
        fixture
            .db
            .mark_receiver_delivery_io_started(&claim, 2_100)
            .expect("mark compatibility provider IO")
    );
    assert_eq!(
        fixture
            .db
            .apply_receiver_delivery_result(
                &claim,
                2_200,
                ReceiverProviderResultClass::Acknowledged(
                    ReceiverProviderReference::parse(
                        "SM0123456789abcdef0123456789abcdef",
                    )
                    .expect("Twilio provider reference"),
                ),
            )
            .expect("deliver preserved compatibility answer"),
        ReceiverDeliveryApplyOutcome::Applied
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load delivered compatibility job")
            .expect("compatibility job remains")
            .state(),
        ReceiverJobState::Done
    );
    let frozen_after = fixture
        .db
        .conn
        .query_row::<String, _, _>(
            "SELECT envelope_json FROM receiver_deliveries",
            [],
            |row| row.get(0),
        )
        .expect("frozen delivery after compatibility repair");
    assert!(
        private_text_proof(&frozen_after) == private_text_proof(&frozen_before),
        "compatibility repair changed the frozen delivery proof"
    );
}

#[test]
fn v12_repair_preserves_but_terminalizes_a_legacy_envelope_without_frozen_sender() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record durable answer")
        .expect("exact answer owner");
    let legacy = r#"{"channel":"sms","value":{"recipient":"+12125550199","body":"private answer","long_form_available":false}}"#;
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_deliveries SET envelope_json = ?2 WHERE job_id = ?1",
            rusqlite::params![fixture.job_id.to_string(), legacy],
        )
        .expect("stage pre-frozen-sender delivery");

    super::super::schema::up(&fixture.db.conn, 12).expect("repair legacy frozen envelope");

    let repaired: (String, Option<String>, String) = fixture
        .db
        .conn
        .query_row(
            "SELECT state, error_category, envelope_json
             FROM receiver_deliveries WHERE job_id = ?1",
            [fixture.job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("load repaired legacy envelope");
    assert_eq!(repaired.0, "failed");
    assert_eq!(repaired.1.as_deref(), Some("invalid-request"));
    assert!(
        private_text_proof(&repaired.2) == private_text_proof(legacy),
        "repair changed the private envelope proof"
    );
    assert_eq!(
        fixture
            .db
            .receiver_job(fixture.job_id)
            .expect("load repaired legacy job")
            .expect("legacy job remains")
            .state(),
        ReceiverJobState::Failed
    );
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
fn v12_repair_recognizes_a_pre_fence_cleanup_whose_session_was_already_released() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    fixture
        .db
        .conn
        .execute(
            "UPDATE receiver_answer_cleanups SET session_released = 1
             WHERE job_id = ?1",
            [fixture.job_id.to_string()],
        )
        .expect("stage progressed pre-fence cleanup");
    fixture
        .db
        .conn
        .execute("DELETE FROM receiver_session_registrations", [])
        .expect("stage discharged registration authority");
    fixture
        .db
        .conn
        .execute("UPDATE brain_sessions SET locked_pid = NULL", [])
        .expect("stage discharged session lock");
    fixture
        .db
        .conn
        .execute_batch(
            "ALTER TABLE receiver_answer_cleanups
             DROP COLUMN controller_shutdown_acknowledged;",
        )
        .expect("stage pre-handoff answer cleanup table");

    super::super::schema::up(&fixture.db.conn, 12)
        .expect("repair progressed answer cleanup handoff");

    let column: (String, String) = fixture
        .db
        .conn
        .query_row(
            "SELECT name, dflt_value FROM pragma_table_info('receiver_answer_cleanups')
             WHERE name = 'controller_shutdown_acknowledged'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("controller shutdown acknowledgement column");
    assert_eq!(column, ("controller_shutdown_acknowledged".to_owned(), "0".to_owned()));
    let cleanup = fixture
        .db
        .next_receiver_answer_cleanup()
        .expect("load repaired progressed cleanup")
        .expect("released legacy cleanup is eligible");
    assert!(cleanup.controller_shutdown_acknowledged());
    assert!(cleanup.session_released());
    assert!(fixture
        .db
        .mark_receiver_answer_artifacts_removed(&cleanup, 1_700)
        .expect("finish repaired artifacts"));
    let cleanup = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("reload repaired cleanup")
        .expect("repaired cleanup remains");
    assert!(fixture
        .db
        .finish_receiver_answer_cleanup(&cleanup)
        .expect("finish repaired cleanup"));
}

#[test]
fn v12_repair_keeps_an_untouched_pre_fence_cleanup_unacknowledged() {
    let fixture = super::binding::completion_fixture(ReceiverJobState::Processing);
    fixture
        .db
        .complete_receiver_job_with_binding(&fixture.request())
        .expect("record exact answer")
        .expect("exact answer owner");
    fixture
        .db
        .conn
        .execute_batch(
            "ALTER TABLE receiver_answer_cleanups
             DROP COLUMN controller_shutdown_acknowledged;",
        )
        .expect("stage untouched pre-fence cleanup");

    super::super::schema::up(&fixture.db.conn, 12)
        .expect("repair untouched answer cleanup handoff");

    let cleanup = fixture
        .db
        .receiver_answer_cleanup(fixture.job_id)
        .expect("load repaired untouched cleanup")
        .expect("untouched cleanup remains");
    assert!(!cleanup.controller_shutdown_acknowledged());
    assert!(!cleanup.session_released());
    assert!(fixture
        .db
        .next_receiver_answer_cleanup()
        .expect("inspect untouched cleanup fence")
        .is_none());
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
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load repaired job")
            .expect("repaired job remains")
            .state(),
        ReceiverJobState::Failed,
        "a terminal delivery repair must remove the corresponding job from the agent lane"
    );
    assert!(
        db.claim_next_receiver_run("agent-owner", 1_000, 2_000)
            .expect("inspect repaired agent lane")
            .is_none()
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
    assert!(
        transcript == "private portable transcript",
        "downgrade changed the retained transcript"
    );
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
