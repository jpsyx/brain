use anyhow::{Result, bail};
use rusqlite::Connection;

use super::super::{DELIVERY_PREVIOUS_VERSION, VERSION};
use crate::state::Db;

const REQUIRED_CONVERSATION_COLUMNS: &[&str] = &[
    "conversation_id",
    "workspace_id",
    "user_id",
    "channel",
    "conversation_key",
    "transcript_markdown",
    "agent_kind",
    "agent_session_id",
    "created_at_unix_ms",
    "updated_at_unix_ms",
];

const REQUIRED_JOB_COLUMNS: &[&str] = &[
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

const REQUIRED_REGISTRATION_COLUMNS: &[&str] = &[
    "workspace_id",
    "conversation_id",
    "agent_kind",
    "actor_id",
    "channel",
    "brain_instance_id",
    "registered_session_id",
    "actual_session_id",
];

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
                 AND (delivery.state != 'acknowledged'
                   OR delivery.provider_reference IS NULL
                   OR length(trim(delivery.provider_reference)) = 0)
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
                 AND (delivery.state != 'acknowledged'
                   OR delivery.provider_reference IS NULL
                   OR length(trim(delivery.provider_reference)) = 0)
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
    validate_table_columns(
        connection,
        "receiver_conversations",
        REQUIRED_CONVERSATION_COLUMNS,
    )?;
    validate_table_columns(connection, "receiver_jobs", REQUIRED_JOB_COLUMNS)?;
    validate_table_columns(
        connection,
        "receiver_session_registrations",
        REQUIRED_REGISTRATION_COLUMNS,
    )?;
    Ok(())
}

fn validate_table_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<()> {
    if !table_exists(connection, table)? {
        bail!("receiver v11 schema is missing required table {table}");
    }
    for column in required_columns {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
            rusqlite::params![table, column],
            |row| row.get(0),
        )?;
        if !exists {
            bail!("receiver v11 schema is missing required {table} column {column}");
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
