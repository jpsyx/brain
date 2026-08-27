use anyhow::{Result, bail};
use rusqlite::Connection;

use super::{DELIVERY_PREVIOUS_VERSION, VERSION};
use crate::state::Db;

const REQUIRED_JOB_COLUMNS: &[&str] = &[
    "job_id",
    "job_token",
    "workspace_id",
    "conversation_id",
    "channel",
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
    "attempt_kind",
    "pending_unavailable_notice",
    "unavailable_notice_owner",
    "unavailable_notice_expires_at_unix_ms",
];

pub(super) fn ensure_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS receiver_deliveries (
           delivery_id                 TEXT PRIMARY KEY,
           job_id                      TEXT NOT NULL REFERENCES receiver_jobs(job_id) ON DELETE CASCADE,
           job_token                   TEXT NOT NULL,
           response_kind               TEXT NOT NULL CHECK (response_kind IN (
             'final-answer', 'unavailable-notice', 'control-acknowledgement', 'fallback-notice'
           )),
           envelope_json               TEXT NOT NULL,
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
           CHECK (state != 'acknowledged' OR provider_reference IS NOT NULL),
           CHECK (state != 'ambiguous' OR ambiguity_reason IS NOT NULL)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS receiver_deliveries_job_kind
           ON receiver_deliveries(job_id, response_kind);
         CREATE INDEX IF NOT EXISTS receiver_deliveries_due
           ON receiver_deliveries(state, retry_at_unix_ms, created_at_unix_ms, delivery_id);",
    )?;
    ensure_optional_columns(connection)?;
    reconcile_rows(connection)?;
    Ok(())
}

fn ensure_optional_columns(connection: &Connection) -> Result<()> {
    for required in [
        "delivery_id",
        "job_id",
        "job_token",
        "response_kind",
        "envelope_json",
        "state",
        "attempt_count",
        "created_at_unix_ms",
        "updated_at_unix_ms",
    ] {
        if !has_delivery_column(connection, required)? {
            bail!("receiver delivery schema is missing required column {required}");
        }
    }
    for (column, definition) in [
        ("attempt_id", "TEXT"),
        ("retry_at_unix_ms", "INTEGER"),
        ("claim_owner", "TEXT"),
        ("claim_expires_at_unix_ms", "INTEGER"),
        ("first_attempt_at_unix_ms", "INTEGER"),
        ("provider_reference", "TEXT"),
        ("error_category", "TEXT"),
        ("ambiguity_reason", "TEXT"),
    ] {
        if !has_delivery_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_deliveries ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS receiver_deliveries_job_kind
           ON receiver_deliveries(job_id, response_kind);
         CREATE INDEX IF NOT EXISTS receiver_deliveries_due
           ON receiver_deliveries(state, retry_at_unix_ms, created_at_unix_ms, delivery_id);",
    )?;
    Ok(())
}

fn reconcile_rows(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "UPDATE receiver_deliveries
         SET state = 'ambiguous', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, ambiguity_reason = 'result-commit-unknown',
             error_category = NULL
         WHERE (claim_owner IS NULL) != (claim_expires_at_unix_ms IS NULL)
            OR (state = 'delivering' AND (
              attempt_id IS NULL OR claim_owner IS NULL
              OR first_attempt_at_unix_ms IS NULL
            ));
         UPDATE receiver_deliveries
         SET claim_owner = NULL, claim_expires_at_unix_ms = NULL
         WHERE state != 'delivering';
         UPDATE receiver_deliveries
         SET retry_at_unix_ms = NULL
         WHERE state != 'retrying';
         UPDATE receiver_deliveries
         SET state = 'failed', error_category = 'invalid-request'
         WHERE state = 'retrying' AND retry_at_unix_ms IS NULL;
         UPDATE receiver_deliveries
         SET state = 'ambiguous', ambiguity_reason = 'provider-acknowledgement-malformed'
         WHERE state = 'acknowledged' AND provider_reference IS NULL;
         UPDATE receiver_deliveries
         SET ambiguity_reason = 'result-commit-unknown'
         WHERE state = 'ambiguous' AND ambiguity_reason IS NULL;",
    )?;
    Ok(())
}

fn has_delivery_column(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_deliveries') WHERE name = ?1
         )",
        [name],
        |row| row.get(0),
    )?)
}

pub(crate) fn down_path(path: &std::path::Path) -> Result<()> {
    down_path_inner(path, None)
}

#[cfg(test)]
pub(in crate::state::receiver) fn down_path_with_busy_observer(
    path: &std::path::Path,
    observer: fn(i32) -> bool,
) -> Result<()> {
    down_path_inner(path, Some(observer))
}

fn down_path_inner(path: &std::path::Path, busy_observer: Option<fn(i32) -> bool>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    Db::configure(&connection)?;
    if let Some(observer) = busy_observer {
        connection.busy_handler(Some(observer))?;
    }
    let transaction = rusqlite::Transaction::new_unchecked(
        &connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    let version: i32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != VERSION {
        transaction.commit()?;
        return Ok(());
    }
    validate_previous_schema(&transaction)?;
    if table_exists(&transaction, "receiver_deliveries")? {
        transaction.execute_batch(
            "UPDATE receiver_jobs
             SET state = 'done', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL
             WHERE EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
             )
             AND NOT EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
                 AND delivery.state != 'acknowledged'
             );
             UPDATE receiver_jobs
             SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 last_error = 'downgrade-no-replay', pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL
             WHERE EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
                 AND delivery.state != 'acknowledged'
             );",
        )?;
    }
    transaction.execute_batch(
        "DROP INDEX IF EXISTS receiver_deliveries_due;
         DROP INDEX IF EXISTS receiver_deliveries_job_kind;
         DROP TABLE IF EXISTS receiver_deliveries;",
    )?;
    transaction.pragma_update(None, "user_version", DELIVERY_PREVIOUS_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn validate_previous_schema(connection: &Connection) -> Result<()> {
    for table in [
        "receiver_conversations",
        "receiver_jobs",
        "receiver_session_registrations",
    ] {
        if !table_exists(connection, table)? {
            bail!("receiver v11 schema is missing required table {table}");
        }
    }
    for column in REQUIRED_JOB_COLUMNS {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM pragma_table_info('receiver_jobs') WHERE name = ?1
             )",
            [column],
            |row| row.get(0),
        )?;
        if !exists {
            bail!("receiver v11 schema is missing required job column {column}");
        }
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
         )",
        [table],
        |row| row.get(0),
    )?)
}
