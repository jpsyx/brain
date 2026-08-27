use anyhow::Result;
use rusqlite::Connection;

use super::{LAUNCH_VERSION, OBSERVATION_VERSION, REGISTRATION_VERSION, VERSION};

pub(crate) fn down_observation_to_registration_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != OBSERVATION_VERSION {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "DROP INDEX IF EXISTS receiver_jobs_ready;
         DROP INDEX IF EXISTS receiver_jobs_job_token;
         ALTER TABLE receiver_jobs RENAME TO receiver_jobs_v9;
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
         SELECT job_id, workspace_id, conversation_id, channel, provider_id, inbound_json,
            CASE WHEN state IN (
              'launching', 'launched', 'accepted', 'processing',
              'answer-ready', 'delivering', 'retrying'
            ) THEN 'failed' ELSE state END,
            received_at_unix_ms, updated_at_unix_ms,
            CASE WHEN state IN (
              'launching', 'launched', 'accepted', 'processing',
              'answer-ready', 'delivering', 'retrying'
            ) THEN NULL ELSE claim_owner END,
            CASE WHEN state IN (
              'launching', 'launched', 'accepted', 'processing',
              'answer-ready', 'delivering', 'retrying'
            ) THEN NULL ELSE claim_expires_at_unix_ms END,
            retry_count,
            CASE WHEN state IN (
              'launching', 'launched', 'accepted', 'processing',
              'answer-ready', 'delivering', 'retrying'
            ) THEN NULL ELSE retry_at_unix_ms END,
            CASE WHEN state IN (
              'launching', 'launched', 'accepted', 'processing',
              'answer-ready', 'delivering', 'retrying'
            ) THEN NULL ELSE retry_from_state END,
            CASE WHEN state IN (
              'launching', 'launched', 'accepted', 'processing',
              'answer-ready', 'delivering', 'retrying'
            ) THEN 'downgrade-no-replay' ELSE last_error END
         FROM receiver_jobs_v9;
         DROP TABLE receiver_jobs_v9;
         CREATE INDEX receiver_jobs_ready ON receiver_jobs(state, retry_at_unix_ms, received_at_unix_ms, job_id);",
    )?;
    transaction.pragma_update(None, "user_version", REGISTRATION_VERSION)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn down_to_previous_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != LAUNCH_VERSION {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch("ALTER TABLE receiver_jobs DROP COLUMN retry_from_state;")?;
    transaction.pragma_update(None, "user_version", LAUNCH_VERSION - 1)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn down_registration_to_launch_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != REGISTRATION_VERSION {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch("DROP TABLE IF EXISTS receiver_session_registrations;")?;
    transaction.pragma_update(None, "user_version", LAUNCH_VERSION)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn down_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.execute_batch(
        "DROP TABLE IF EXISTS receiver_session_registrations;
         DROP INDEX IF EXISTS receiver_jobs_ready;
         DROP TABLE IF EXISTS receiver_jobs;
         DROP TABLE IF EXISTS receiver_conversations;",
    )?;
    let version: i32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if matches!(version, VERSION | OBSERVATION_VERSION | LAUNCH_VERSION | 6) {
        transaction.pragma_update(None, "user_version", 5)?;
    }
    transaction.commit()?;
    Ok(())
}
