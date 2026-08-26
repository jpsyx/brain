#[test]
fn receiver_schema_enforces_conversation_foreign_keys() {
    let db = Db::open_in_memory().expect("receiver state");
    let enabled: i64 = db
        .conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("foreign key setting");
    let job = receiver_job(None, 100);
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&job, &identity)
        .expect("accept receiver job");

    assert_eq!(enabled, 1);
    assert!(
        db.conn
            .execute(
                "DELETE FROM receiver_conversations WHERE conversation_id = ?1",
                [accepted.conversation_id().to_string()],
            )
            .is_err()
    );
}

#[test]
fn v6_upgrade_repairs_missing_receiver_state_before_advancing_to_v9() {
    let db = Db::open_in_memory().expect("receiver state");
    db.conn
        .execute_batch("DROP TABLE receiver_jobs; PRAGMA user_version = 6;")
        .expect("seed partial v6 schema");

    super::super::schema::up(&db.conn, 6).expect("repair and upgrade receiver schema");

    let version: i64 = db
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("state schema version");
    let retry_origin_columns: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('receiver_jobs')
             WHERE name = 'retry_from_state'",
            [],
            |row| row.get(0),
        )
        .expect("receiver retry-origin column count");
    let registration_tables: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'receiver_session_registrations'",
            [],
            |row| row.get(0),
        )
        .expect("receiver registration table count");
    assert_eq!(version, 9);
    assert_eq!(retry_origin_columns, 1);
    assert_eq!(registration_tables, 1);
}

fn stage_v8_receiver_jobs(db: &Db) {
    db.conn
        .execute_batch(
            "DROP INDEX IF EXISTS receiver_jobs_ready;
             ALTER TABLE receiver_jobs RENAME TO receiver_jobs_current;
             CREATE TABLE receiver_jobs (
               job_id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL,
               conversation_id TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
               channel TEXT NOT NULL CHECK (channel IN ('sms', 'email')), provider_id TEXT,
               inbound_json TEXT NOT NULL,
               state TEXT NOT NULL CHECK (state IN ('queued', 'claimed', 'launching', 'accepted', 'processing', 'answer-ready', 'delivering', 'retrying', 'failed', 'done')),
               received_at_unix_ms INTEGER NOT NULL, updated_at_unix_ms INTEGER NOT NULL,
               claim_owner TEXT, claim_expires_at_unix_ms INTEGER,
               retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0), retry_at_unix_ms INTEGER,
               retry_from_state TEXT CHECK (retry_from_state IN ('claimed', 'launching', 'accepted', 'processing', 'delivering')),
               last_error TEXT, UNIQUE (workspace_id, channel, provider_id),
               CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL))
             );
             INSERT INTO receiver_jobs
               (job_id, workspace_id, conversation_id, channel, provider_id, inbound_json, state,
                received_at_unix_ms, updated_at_unix_ms, claim_owner, claim_expires_at_unix_ms,
                retry_count, retry_at_unix_ms, retry_from_state, last_error)
             SELECT job_id, workspace_id, conversation_id, channel, provider_id, inbound_json, state,
                received_at_unix_ms, updated_at_unix_ms, claim_owner, claim_expires_at_unix_ms,
                retry_count, retry_at_unix_ms, retry_from_state, last_error
             FROM receiver_jobs_current;
             DROP TABLE receiver_jobs_current;
             CREATE INDEX receiver_jobs_ready
               ON receiver_jobs(state, retry_at_unix_ms, received_at_unix_ms, job_id);
             PRAGMA user_version = 8;",
        )
        .expect("stage v8 receiver jobs");
}

fn token_column_is_not_null(db: &Db) -> bool {
    db.conn
        .query_row(
            "SELECT \"notnull\" FROM pragma_table_info('receiver_jobs') WHERE name = 'job_token'",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("job token column")
}

#[test]
fn v8_upgrade_preserves_job_ids_and_inbound_json_while_generating_distinct_tokens() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let first = db
        .accept_receiver_job(&receiver_job(Some("v8-first"), 100), &identity)
        .expect("accept first job");
    let second = db
        .accept_receiver_job(&receiver_job(Some("v8-second"), 200), &identity)
        .expect("accept second job");
    let before = [first.job_id(), second.job_id()]
        .into_iter()
        .map(|job_id| {
            db.conn
                .query_row(
                    "SELECT job_id, CAST(inbound_json AS BLOB) FROM receiver_jobs WHERE job_id = ?1",
                    [job_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .expect("v8 source row")
        })
        .collect::<Vec<_>>();
    stage_v8_receiver_jobs(&db);

    super::super::schema::up(&db.conn, 8).expect("upgrade v8 receiver jobs");

    let after = db
        .conn
        .prepare("SELECT job_id, CAST(inbound_json AS BLOB), job_token FROM receiver_jobs ORDER BY job_id")
        .expect("load upgraded jobs")
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query upgraded jobs")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect upgraded jobs");
    assert_eq!(
        after
            .iter()
            .map(|(job_id, inbound_json, _)| (job_id.clone(), inbound_json.clone()))
            .collect::<Vec<_>>(),
        {
            let mut expected = before;
            expected.sort_by(|left, right| left.0.cmp(&right.0));
            expected
        }
    );
    assert_ne!(after[0].2, after[1].2);
    for (_, _, token) in &after {
        ReceiverJobToken::parse(token).expect("valid migrated token");
    }
    assert!(db
        .conn
        .execute(
            "UPDATE receiver_jobs SET job_token = ?1 WHERE job_id = ?2",
            [&after[0].2, &after[1].0],
        )
        .is_err());
}

#[test]
fn partial_v8_state_check_with_job_token_is_rebuilt_to_the_v9_contract() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    db.accept_receiver_job(&receiver_job(Some("partial-v8"), 100), &identity)
        .expect("accept receiver job");
    stage_v8_receiver_jobs(&db);
    db.conn
        .execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN job_token TEXT;
             UPDATE receiver_jobs SET job_token = '00000000-0000-4000-8000-000000000001';",
        )
        .expect("stage partial v8 token column");

    super::super::schema::up(&db.conn, 8).expect("repair partial v8 receiver jobs");

    assert!(db
        .conn
        .execute("UPDATE receiver_jobs SET state = 'launched'", [])
        .is_ok());
    assert!(token_column_is_not_null(&db));
}

#[test]
fn damaged_v9_token_contract_is_rebuilt_but_current_v9_rows_reconcile_idempotently() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let accepted = db
        .accept_receiver_job(&receiver_job(Some("damaged-v9"), 100), &identity)
        .expect("accept receiver job");
    let original_token = db
        .receiver_job(accepted.job_id())
        .expect("load receiver job")
        .expect("receiver job")
        .token()
        .to_string();
    let current_sql: String = db
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'receiver_jobs'",
            [],
            |row| row.get(0),
        )
        .expect("current receiver schema");
    let damaged_sql = current_sql.replacen(
        "job_token                 TEXT NOT NULL UNIQUE",
        "job_token                 TEXT",
        1,
    );
    assert_ne!(damaged_sql, current_sql, "stage must remove token contract");
    db.conn
        .execute_batch(&format!(
            "DROP INDEX IF EXISTS receiver_jobs_ready;
             ALTER TABLE receiver_jobs RENAME TO receiver_jobs_current;
             {damaged_sql};
             INSERT INTO receiver_jobs SELECT * FROM receiver_jobs_current;
             DROP TABLE receiver_jobs_current;
             PRAGMA user_version = 9;"
        ))
        .expect("stage damaged v9 receiver jobs");

    super::super::schema::up(&db.conn, 9).expect("repair damaged v9 receiver jobs");

    assert!(token_column_is_not_null(&db));
    assert_eq!(
        db.receiver_job(accepted.job_id())
            .expect("load reconciled receiver job")
            .expect("receiver job")
            .token()
            .to_string(),
        original_token
    );
    super::super::schema::up(&db.conn, 9).expect("reconcile already-current v9 schema");
    assert!(token_column_is_not_null(&db));
}

#[test]
fn damaged_v9_reconciliation_preserves_every_observation_field() {
    let db = Db::open_in_memory().expect("receiver state");
    let identity = ReceiverConversationIdentity::sms(receiver_workspace_id(), receiver_user_id());
    let jobs = [
        ("damaged-launched", "launched", 11_i64),
        ("damaged-accepted", "accepted", 12_i64),
        ("damaged-processing", "processing", 13_i64),
    ]
    .into_iter()
    .map(|(provider_id, state, revision)| {
        let accepted = db
            .accept_receiver_job(&receiver_job(Some(provider_id), 100), &identity)
            .expect("accept receiver job");
        db.conn
            .execute(
                "UPDATE receiver_jobs
                 SET state = ?1, launched_at_unix_ms = ?2, accepted_at_unix_ms = ?3,
                     progressing_at_unix_ms = ?4, completed_at_unix_ms = ?5,
                     observation_instance = ?6, observation_session_id = ?7,
                     observation_revision = ?8
                 WHERE job_id = ?9",
                rusqlite::params![
                    state,
                    1_000 + revision,
                    2_000 + revision,
                    3_000 + revision,
                    4_000 + revision,
                    format!("instance-{revision}"),
                    format!("session-{revision}"),
                    revision,
                    accepted.job_id().to_string(),
                ],
            )
            .expect("seed v9 observation evidence");
        accepted.job_id()
    })
    .collect::<Vec<_>>();
    let evidence = |db: &Db| {
        let mut statement = db
            .conn
            .prepare(
                "SELECT job_id, job_token, launched_at_unix_ms, accepted_at_unix_ms,
                        progressing_at_unix_ms, completed_at_unix_ms, observation_instance,
                        observation_session_id, observation_revision
                 FROM receiver_jobs ORDER BY job_id",
            )
            .expect("prepare observation evidence query");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })
            .expect("query observation evidence")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect observation evidence")
    };
    let before = evidence(&db);
    let current_sql: String = db
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'receiver_jobs'",
            [],
            |row| row.get(0),
        )
        .expect("current receiver schema");
    let damaged_sql = current_sql.replacen(
        "job_token                 TEXT NOT NULL UNIQUE",
        "job_token                 TEXT",
        1,
    );
    db.conn
        .execute_batch(&format!(
            "DROP INDEX IF EXISTS receiver_jobs_ready;
             ALTER TABLE receiver_jobs RENAME TO receiver_jobs_current;
             {damaged_sql};
             INSERT INTO receiver_jobs SELECT * FROM receiver_jobs_current;
             DROP TABLE receiver_jobs_current;
             PRAGMA user_version = 9;"
        ))
        .expect("stage damaged v9 receiver jobs");

    super::super::schema::up(&db.conn, 9).expect("repair damaged v9 receiver jobs");

    assert_eq!(evidence(&db), before);
    super::super::schema::up(&db.conn, 9).expect("reconcile already-current v9 receiver jobs");
    assert_eq!(evidence(&db), before);
    for job_id in jobs {
        assert!(db
            .receiver_job(job_id)
            .expect("load reconciled receiver job")
            .is_some());
    }
}
