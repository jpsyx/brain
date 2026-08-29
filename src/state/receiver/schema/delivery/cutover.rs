use anyhow::Result;
use rusqlite::Connection;

use crate::state::Db;

const SOURCE_VERSION: i32 = 13;
const TARGET_VERSION: i32 = 12;

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
    connection.pragma_update(None, "foreign_keys", false)?;
    let transaction = rusqlite::Transaction::new_unchecked(
        &connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    let version: i32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SOURCE_VERSION {
        transaction.commit()?;
        return Ok(());
    }
    ensure_v12_job_columns(&transaction)?;
    let had_deliveries = table_exists(&transaction, "receiver_deliveries")?;
    if had_deliveries {
        super::ensure_schema(&transaction)?;
        transaction.execute_batch(
            "UPDATE receiver_jobs
             SET state = 'failed', pending_unavailable_notice = 1,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL
             WHERE EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
                 AND delivery.job_token = receiver_jobs.job_token
                 AND delivery.response_kind = 'unavailable-notice'
                 AND delivery.state = 'cleanup-gated'
             );
             DELETE FROM receiver_deliveries WHERE state = 'cleanup-gated';",
        )?;
        stage_v12_deliveries(&transaction)?;
    }
    transaction.pragma_update(None, "legacy_alter_table", true)?;
    super::super::job_contract::rebuild_exact_v12(&transaction)?;
    transaction.pragma_update(None, "legacy_alter_table", false)?;
    restore_v12_deliveries(&transaction, had_deliveries)?;
    let has_foreign_key_violation: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
        [],
        |row| row.get(0),
    )?;
    anyhow::ensure!(
        !has_foreign_key_violation,
        "receiver v13 downgrade would violate foreign-key authority"
    );
    transaction.pragma_update(None, "user_version", TARGET_VERSION)?;
    transaction.commit()?;
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn stage_v12_deliveries(connection: &Connection) -> Result<()> {
    super::indexes::reject_duplicate_semantic_responses(connection)?;
    connection.execute_batch(
        "DROP INDEX IF EXISTS receiver_deliveries_due;
         DROP INDEX IF EXISTS receiver_deliveries_job_kind;
         CREATE TEMP TABLE receiver_deliveries_v13_snapshot AS
           SELECT * FROM receiver_deliveries;
         DROP TABLE receiver_deliveries;",
    )?;
    Ok(())
}

fn restore_v12_deliveries(connection: &Connection, had_snapshot: bool) -> Result<()> {
    connection.execute_batch(super::contract::CREATE_V12_TABLE)?;
    if had_snapshot {
        connection.execute_batch(
            "INSERT INTO receiver_deliveries
           (delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, frozen_fallbacks_json, state,
            attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, provider_io_started, first_attempt_at_unix_ms,
            provider_reference, error_category, ambiguity_reason, fallback_decision,
            created_at_unix_ms, updated_at_unix_ms)
         SELECT delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, frozen_fallbacks_json, state,
            attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, provider_io_started, first_attempt_at_unix_ms,
            provider_reference, error_category, ambiguity_reason, fallback_decision,
            created_at_unix_ms, updated_at_unix_ms
         FROM receiver_deliveries_v13_snapshot;
         DROP TABLE receiver_deliveries_v13_snapshot;",
        )?;
    }
    super::indexes::ensure_managed(connection)?;
    Ok(())
}

fn ensure_v12_job_columns(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        (
            "pending_unavailable_notice",
            "INTEGER NOT NULL DEFAULT 0 CHECK (pending_unavailable_notice IN (0, 1))",
        ),
        ("unavailable_notice_owner", "TEXT"),
        ("unavailable_notice_expires_at_unix_ms", "INTEGER"),
    ] {
        if !super::super::has_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_jobs ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET unavailable_notice_owner = NULL,
             unavailable_notice_expires_at_unix_ms = NULL
         WHERE (unavailable_notice_owner IS NULL)
            != (unavailable_notice_expires_at_unix_ms IS NULL);",
    )?;
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )?)
}
