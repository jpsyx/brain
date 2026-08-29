fn stored_receiver_table_sql(connection: &rusqlite::Connection, table: &str) -> String {
    connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .expect("stored receiver table SQL")
}

fn replace_empty_receiver_table(
    connection: &rusqlite::Connection,
    table: &str,
    original: &str,
    damaged: &str,
) {
    assert!(
        original != damaged,
        "receiver table mutation must change the contract"
    );
    connection
        .execute_batch(&format!("DROP TABLE {table}; {damaged};"))
        .expect("replace empty receiver table with damaged contract");
}

#[test]
fn v12_repair_fingerprints_every_delivery_table_invariant_and_survives_down_up() {
    let mutations = [
        (
            "job_id                      TEXT NOT NULL REFERENCES receiver_jobs(job_id) ON DELETE CASCADE",
            "job_id                      TEXT NOT NULL",
        ),
        (
            "'ready', 'delivering', 'retrying', 'acknowledged', 'failed', 'ambiguous'",
            "'ready', 'delivering', 'retrying', 'acknowledged', 'failed', 'ambiguous', 'damaged'",
        ),
        (
            "UNIQUE (job_id, response_kind),",
            "CHECK (length(job_id) > 0),",
        ),
        (
            "CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL)),",
            "CHECK (claim_expires_at_unix_ms IS NULL OR claim_expires_at_unix_ms >= 0),",
        ),
        (
            "CHECK (state = 'delivering' OR provider_io_started = 0),",
            "CHECK (provider_io_started IN (0, 1)),",
        ),
    ];
    for (case_index, (needle, replacement)) in mutations.into_iter().enumerate() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("state.db");
        let db = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("receiver state");
        let canonical = stored_receiver_table_sql(&db.conn, "receiver_deliveries");
        let damaged = canonical.replacen(needle, replacement, 1);
        db.conn
            .execute_batch(
                "DROP INDEX receiver_deliveries_due;
                 DROP INDEX receiver_deliveries_job_kind;",
            )
            .expect("drop managed delivery indexes");
        replace_empty_receiver_table(
            &db.conn,
            "receiver_deliveries",
            &canonical,
            &damaged,
        );
        db.conn
            .execute_batch(
                "CREATE UNIQUE INDEX receiver_deliveries_job_kind
                   ON receiver_deliveries(job_id, response_kind);
                 CREATE INDEX receiver_deliveries_due
                   ON receiver_deliveries(
                     state, retry_at_unix_ms, created_at_unix_ms, delivery_id
                   );",
            )
            .expect("retain canonical managed delivery indexes");
        assert!(
            damaged.contains(
                "state NOT IN ('failed', 'ambiguous') OR fallback_decision IS NOT NULL"
            ),
            "case {case_index} must retain the previously checked contract fragment"
        );

        super::super::schema::up(&db.conn, 12).expect("repair full delivery table contract");
        assert!(
            stored_receiver_table_sql(&db.conn, "receiver_deliveries") == canonical,
            "case {case_index} retained a damaged delivery table contract"
        );
        super::super::schema::up(&db.conn, 12).expect("repeat delivery contract repair");
        assert!(
            stored_receiver_table_sql(&db.conn, "receiver_deliveries") == canonical,
            "case {case_index} was not idempotent"
        );
        drop(db);

        super::super::schema::down_delivery_path(&path).expect("downgrade repaired delivery schema");
        let reopened = Db::open_path_with_legacy_identity(
            &path,
            &receiver_workspace_id().to_string(),
            receiver_user_id().as_str(),
        )
        .expect("re-upgrade repaired delivery schema");
        assert!(
            stored_receiver_table_sql(&reopened.conn, "receiver_deliveries") == canonical,
            "case {case_index} did not preserve the canonical down/up contract"
        );
    }
}

#[test]
fn v12_repair_fingerprints_every_answer_cleanup_table_invariant() {
    let mutations = [
        (
            "job_id                  TEXT PRIMARY KEY REFERENCES receiver_jobs(job_id) ON DELETE CASCADE",
            "job_id                  TEXT PRIMARY KEY",
        ),
        (
            "agent_kind              TEXT NOT NULL CHECK (agent_kind IN ('claude', 'codex', 'opencode'))",
            "agent_kind              TEXT NOT NULL",
        ),
        (
            "channel                 TEXT NOT NULL CHECK (channel IN ('sms', 'email'))",
            "channel                 TEXT NOT NULL",
        ),
        (
            "CHECK (controller_shutdown_acknowledged IN (0, 1))",
            "CHECK (controller_shutdown_acknowledged >= 0)",
        ),
        (
            "session_released        INTEGER NOT NULL DEFAULT 0 CHECK (session_released IN (0, 1))",
            "session_released        INTEGER NOT NULL DEFAULT 0",
        ),
    ];
    for (case_index, (needle, replacement)) in mutations.into_iter().enumerate() {
        let db = Db::open_in_memory().expect("receiver state");
        let canonical = stored_receiver_table_sql(&db.conn, "receiver_answer_cleanups");
        let damaged = canonical.replacen(needle, replacement, 1);
        replace_empty_receiver_table(
            &db.conn,
            "receiver_answer_cleanups",
            &canonical,
            &damaged,
        );

        super::super::schema::up(&db.conn, 12).expect("repair full cleanup table contract");
        assert!(
            stored_receiver_table_sql(&db.conn, "receiver_answer_cleanups") == canonical,
            "case {case_index} retained a damaged cleanup table contract"
        );
        super::super::schema::up(&db.conn, 12).expect("repeat cleanup contract repair");
        assert!(
            stored_receiver_table_sql(&db.conn, "receiver_answer_cleanups") == canonical,
            "case {case_index} cleanup repair was not idempotent"
        );
    }
}
