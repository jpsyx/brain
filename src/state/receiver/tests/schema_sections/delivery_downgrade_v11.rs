fn assert_malformed_v11_shape_rolls_back_delivery_downgrade(alter_schema: &str) {
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
                &receiver_job(Some("private-malformed-v11"), 100),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs SET state = 'answer-ready' WHERE job_id = ?1",
                [accepted.job_id().to_string()],
            )
            .expect("stage answer-ready job");
        seed_delivery_row(
            &db,
            accepted.job_id(),
            persisted_job_token(&db, accepted.job_id()),
            "10000000-0000-4000-8000-000000000001",
            "final-answer",
            "ready",
            r#"{"channel":"sms","value":{"recipient":"+12125550199","body":"private answer","long_form_available":false}}"#,
        );
        db.conn
            .execute_batch(alter_schema)
            .expect("damage one v11 schema requirement");
        accepted.job_id().to_string()
    };

    let error = super::super::schema::down_delivery_path(&path)
        .expect_err("malformed v11 schema must block downgrade");

    let connection = rusqlite::Connection::open(path).expect("unchanged v12 state");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("retained version");
    let outbox_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM receiver_deliveries", [], |row| {
            row.get(0)
        })
        .expect("retained outbox rows");
    let job_state: String = connection
        .query_row(
            "SELECT state FROM receiver_jobs WHERE job_id = ?1",
            [job_id],
            |row| row.get(0),
        )
        .expect("retained job state");

    assert_eq!(version, 12);
    assert_eq!(outbox_rows, 1);
    assert_eq!(job_state, "answer-ready");
    assert!(!error.to_string().contains("private"));
}

#[derive(Clone, Copy)]
enum V11ContractDamage {
    ChannelCheck,
    ProviderUniqueness,
    ConversationForeignKey,
    ManagedIndexes,
}

fn damage_receiver_jobs_contract(connection: &rusqlite::Connection, damage: V11ContractDamage) {
    if matches!(damage, V11ContractDamage::ManagedIndexes) {
        connection
            .execute_batch(
                "DROP INDEX IF EXISTS receiver_jobs_ready;
                 DROP INDEX IF EXISTS receiver_jobs_job_token;",
            )
            .expect("remove managed v11 indexes");
        return;
    }
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_jobs'",
            [],
            |row| row.get(0),
        )
        .expect("load receiver jobs contract");
    let damaged = match damage {
        V11ContractDamage::ChannelCheck => sql.replace(
            "channel                   TEXT NOT NULL CHECK (channel IN ('sms', 'email'))",
            "channel                   TEXT NOT NULL",
        ),
        V11ContractDamage::ProviderUniqueness => {
            sql.replace("UNIQUE (workspace_id, channel, provider_id),", "")
        }
        V11ContractDamage::ConversationForeignKey => sql.replace(
            "conversation_id           TEXT NOT NULL REFERENCES receiver_conversations(conversation_id)",
            "conversation_id           TEXT NOT NULL",
        ),
        V11ContractDamage::ManagedIndexes => unreachable!(),
    }
    .replacen(
        "CREATE TABLE receiver_jobs",
        "CREATE TABLE receiver_jobs_damaged",
        1,
    );
    assert!(damaged != sql, "contract mutation did not change the fixture");
    connection
        .execute_batch(&format!(
            "PRAGMA foreign_keys = OFF;
             DROP INDEX IF EXISTS receiver_jobs_ready;
             DROP INDEX IF EXISTS receiver_jobs_job_token;
             {damaged};
             INSERT INTO receiver_jobs_damaged SELECT * FROM receiver_jobs;
             DROP TABLE receiver_jobs;
             ALTER TABLE receiver_jobs_damaged RENAME TO receiver_jobs;
             PRAGMA foreign_keys = ON;"
        ))
        .expect("install damaged receiver jobs contract");
}

fn v11_index_matches(
    connection: &rusqlite::Connection,
    name: &str,
    unique: bool,
    columns: &[&str],
) -> bool {
    let actual_unique = connection
        .query_row(
            "SELECT \"unique\" FROM pragma_index_list('receiver_jobs') WHERE name = ?1",
            [name],
            |row| row.get::<_, bool>(0),
        )
        .ok();
    let actual_columns = connection
        .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
        .and_then(|mut statement| {
            statement
                .query_map([name], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap_or_default();
    actual_unique == Some(unique)
        && actual_columns
            .iter()
            .map(String::as_str)
            .eq(columns.iter().copied())
}

#[test]
fn v12_down_rebuilds_every_v11_job_constraint_and_managed_index() {
    for damage in [
        V11ContractDamage::ChannelCheck,
        V11ContractDamage::ProviderUniqueness,
        V11ContractDamage::ConversationForeignKey,
        V11ContractDamage::ManagedIndexes,
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("state.db");
        {
            let db = Db::open_path_with_legacy_identity(
                &path,
                &receiver_workspace_id().to_string(),
                receiver_user_id().as_str(),
            )
            .expect("receiver state");
            let identity =
                ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
            db.accept_receiver_job(&receiver_job(Some("v11-contract-one"), 100), &identity)
                .expect("accept first retained job");
            db.accept_receiver_job(&receiver_job(Some("v11-contract-two"), 101), &identity)
                .expect("accept second retained job");
            damage_receiver_jobs_contract(&db.conn, damage);
        }

        super::super::schema::down_delivery_path(&path)
            .expect("rebuild exact v11 receiver jobs contract");

        let connection = rusqlite::Connection::open(&path).expect("downgraded v11 state");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable old-reader foreign keys");
        let sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type = 'table' AND name = 'receiver_jobs'",
                [],
                |row| row.get(0),
            )
            .expect("load rebuilt v11 contract");
        assert!(
            sql.contains("CHECK (channel IN ('sms', 'email'))"),
            "v11 channel constraint was not restored"
        );
        assert!(
            sql.contains("REFERENCES receiver_conversations(conversation_id)"),
            "v11 conversation foreign key was not restored"
        );
        assert!(
            sql.contains("UNIQUE (workspace_id, channel, provider_id)"),
            "v11 provider uniqueness was not restored"
        );
        assert!(
            v11_index_matches(
                &connection,
                "receiver_jobs_ready",
                false,
                &[
                    "state",
                    "retry_at_unix_ms",
                    "received_at_unix_ms",
                    "job_id"
                ]
            ),
            "v11 ready index was not restored"
        );
        assert!(
            v11_index_matches(
                &connection,
                "receiver_jobs_job_token",
                true,
                &["job_token"]
            ),
            "v11 token index was not restored"
        );
        assert!(
            connection
                .execute(
                    "UPDATE receiver_jobs SET channel = 'invalid' WHERE provider_id = 'v11-contract-one'",
                    [],
                )
                .is_err(),
            "old reader accepted an invalid channel"
        );
        assert!(
            connection
                .execute(
                    "UPDATE receiver_jobs SET conversation_id = 'missing' WHERE provider_id = 'v11-contract-one'",
                    [],
                )
                .is_err(),
            "old reader accepted an orphan conversation"
        );
        assert!(
            connection
                .execute(
                    "UPDATE receiver_jobs SET provider_id = 'v11-contract-one' WHERE provider_id = 'v11-contract-two'",
                    [],
                )
                .is_err(),
            "old reader accepted a duplicate provider identity"
        );
    }
}

#[test]
fn v12_down_rejects_malformed_rows_without_stamping_or_dropping_v12_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let identity =
            ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
        db.accept_receiver_job(&receiver_job(Some("v11-invalid-row"), 100), &identity)
            .expect("accept retained job");
        let sql: String = db
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type = 'table' AND name = 'receiver_jobs'",
                [],
                |row| row.get(0),
            )
            .expect("load receiver jobs contract");
        let damaged = sql
            .replace("CHECK (retry_count >= 0)", "")
            .replacen(
                "CREATE TABLE receiver_jobs",
                "CREATE TABLE receiver_jobs_damaged",
                1,
            );
        db.conn
            .execute_batch(&format!(
                "PRAGMA foreign_keys = OFF;
                 DROP INDEX IF EXISTS receiver_jobs_ready;
                 DROP INDEX IF EXISTS receiver_jobs_job_token;
                 {damaged};
                 INSERT INTO receiver_jobs_damaged SELECT * FROM receiver_jobs;
                 DROP TABLE receiver_jobs;
                 ALTER TABLE receiver_jobs_damaged RENAME TO receiver_jobs;
                 UPDATE receiver_jobs SET retry_count = -1;
                 PRAGMA foreign_keys = ON;"
            ))
            .expect("stage malformed v12 source row");
    }

    assert!(
        super::super::schema::down_delivery_path(&path).is_err(),
        "malformed v12 source row was stamped as v11"
    );
    let connection = rusqlite::Connection::open(path).expect("rolled-back v12 state");
    let unchanged: (i64, i64, i64, i64) = (
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("retained schema version"),
        connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs') WHERE name = 'response_sender'",
                [],
                |row| row.get(0),
            )
            .expect("retained response sender"),
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'receiver_deliveries'",
                [],
                |row| row.get(0),
            )
            .expect("retained delivery table"),
        connection
            .query_row("SELECT MIN(retry_count) FROM receiver_jobs", [], |row| row.get(0))
            .expect("retained malformed source row"),
    );
    assert!(
        unchanged == (12, 1, 1, -1),
        "failed downgrade did not roll back atomically"
    );
}

#[test]
fn v12_down_requires_the_complete_v11_conversation_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_conversations DROP COLUMN transcript_markdown;",
    );
}

#[test]
fn v12_down_requires_the_complete_v11_job_recovery_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_jobs DROP COLUMN latest_progress_at_unix_ms;",
    );
}

#[test]
fn v12_down_requires_the_complete_v11_job_notice_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_jobs DROP COLUMN unavailable_notice_expires_at_unix_ms;",
    );
}

#[test]
fn v12_down_requires_the_complete_v11_registration_shape() {
    assert_malformed_v11_shape_rolls_back_delivery_downgrade(
        "ALTER TABLE receiver_session_registrations DROP COLUMN actual_session_id;",
    );
}

#[test]
fn v12_down_restores_the_exact_v11_job_shape_and_round_trips_up_safely() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("state.db");
    let (job_id, inbound_proof) = {
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let accepted = db
            .accept_receiver_job(
                &receiver_job(Some("v11-shape-round-trip"), 123),
                &ReceiverConversationIdentity::sms(
                    receiver_workspace_id(),
                    receiver_user_id(),
                ),
            )
            .expect("accept receiver job");
        let inbound_json: String = db
            .conn
            .query_row(
                "SELECT inbound_json FROM receiver_jobs WHERE job_id = ?1",
                [accepted.job_id().to_string()],
                |row| row.get(0),
            )
            .expect("load private inbound frame");
        (accepted.job_id().to_string(), private_text_proof(&inbound_json))
    };

    super::super::schema::down_delivery_path(&path).expect("downgrade exact v11 shape");

    let connection = rusqlite::Connection::open(&path).expect("downgraded state");
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info('receiver_jobs') ORDER BY cid")
        .expect("prepare v11 job columns");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query v11 job columns")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect v11 job columns");
    let expected_columns = [
        "job_id",
        "job_token",
        "workspace_id",
        "conversation_id",
        "channel",
        "provider_id",
        "inbound_json",
        "state",
        "received_at_unix_ms",
        "updated_at_unix_ms",
        "claim_owner",
        "claim_expires_at_unix_ms",
        "retry_count",
        "retry_at_unix_ms",
        "retry_from_state",
        "last_error",
        "launched_at_unix_ms",
        "accepted_at_unix_ms",
        "progressing_at_unix_ms",
        "completed_at_unix_ms",
        "observation_instance",
        "observation_session_id",
        "observation_revision",
        "attempt_accepted_at_unix_ms",
        "attempt_progressing_at_unix_ms",
        "latest_progress_at_unix_ms",
        "launch_expires_at_unix_ms",
        "acceptance_expires_at_unix_ms",
        "progress_expires_at_unix_ms",
        "recovery_expires_at_unix_ms",
        "absolute_work_expires_at_unix_ms",
        "recovery_count",
        "attempt_kind",
        "pending_unavailable_notice",
        "recovery_cleanup_instance",
        "recovery_cleanup_session_id",
        "unavailable_notice_owner",
        "unavailable_notice_expires_at_unix_ms",
    ];
    assert!(
        columns.iter().map(String::as_str).eq(expected_columns),
        "delivery downgrade did not restore the exact v11 job columns"
    );
    let retained: (String, String, i64, i64, String) = connection
        .query_row(
            "SELECT state, inbound_json, received_at_unix_ms, retry_count, attempt_kind
             FROM receiver_jobs WHERE job_id = ?1",
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
        .expect("load retained v11 job");
    assert!(retained.0 == "queued", "valid v11 job state changed");
    assert!(
        private_text_proof(&retained.1) == inbound_proof,
        "private inbound frame changed during downgrade"
    );
    assert!(retained.2 == 123, "valid v11 receive time changed");
    assert!(retained.3 == 0, "valid v11 retry count changed");
    assert!(retained.4 == "ordinary", "valid v11 attempt kind changed");
    let private_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name IN ('receiver_deliveries', 'receiver_answer_cleanups')",
            [],
            |row| row.get(0),
        )
        .expect("count v12 private tables");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("v11 schema version");
    assert!(private_tables == 0, "v12 private tables survived downgrade");
    assert!(version == 11, "delivery downgrade did not finish at v11");
    drop(statement);
    drop(connection);

    let reopened = Db::open_path_with_legacy_identity(
        &path,
        &receiver_workspace_id().to_string(),
        receiver_user_id().as_str(),
    )
    .expect("upgrade the exact v11 state");
    let upgraded: (i64, i64, i64, Option<String>) = (
        reopened
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("upgraded version"),
        reopened
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table'
                   AND name IN ('receiver_deliveries', 'receiver_answer_cleanups')",
                [],
                |row| row.get(0),
            )
            .expect("recreated v12 private tables"),
        reopened
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
                 WHERE name = 'response_sender'",
                [],
                |row| row.get(0),
            )
            .expect("recreated response sender column"),
        reopened
            .conn
            .query_row(
                "SELECT response_sender FROM receiver_jobs WHERE job_id = ?1",
                [&job_id],
                |row| row.get(0),
            )
            .expect("upgraded legacy sender value"),
    );
    assert!(
        upgraded == (12, 2, 1, None),
        "exact v11 state did not upgrade back to repaired v12"
    );
}
