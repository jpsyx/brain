use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _};

use super::ReceiverJobToken;

mod delivery;
mod downgrade;
mod notice;
mod recovery;
mod token;
use token::populate_job_tokens;

pub(super) const VERSION: i32 = 12;
pub(super) const DELIVERY_PREVIOUS_VERSION: i32 = 11;
pub(super) const RECOVERY_VERSION: i32 = 10;
pub(super) const OBSERVATION_VERSION: i32 = 9;
pub(super) const REGISTRATION_VERSION: i32 = 8;
pub(super) const LAUNCH_VERSION: i32 = 7;

pub(in crate::state) fn up(connection: &Connection, current_version: i32) -> Result<()> {
    up_with_token_factory(connection, current_version, ReceiverJobToken::new)
}

pub(super) fn up_with_token_factory(
    connection: &Connection,
    current_version: i32,
    mut next_token: impl FnMut() -> ReceiverJobToken,
) -> Result<()> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)?;
    let had_any_recovery_column = recovery::had_any_column(&transaction);
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS receiver_conversations (
           conversation_id      TEXT PRIMARY KEY,
           workspace_id         TEXT NOT NULL,
           user_id              TEXT NOT NULL,
           channel              TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           conversation_key     TEXT NOT NULL,
           transcript_markdown  TEXT NOT NULL DEFAULT '',
           agent_kind           TEXT,
           agent_session_id     TEXT,
           created_at_unix_ms   INTEGER NOT NULL,
           updated_at_unix_ms   INTEGER NOT NULL,
           UNIQUE (workspace_id, user_id, channel, conversation_key),
           CHECK ((agent_kind IS NULL) = (agent_session_id IS NULL))
         );
         CREATE TABLE IF NOT EXISTS receiver_jobs (
           job_id                    TEXT PRIMARY KEY,
           job_token                 TEXT NOT NULL UNIQUE,
           workspace_id              TEXT NOT NULL,
           conversation_id           TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
           channel                   TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           provider_id               TEXT,
           inbound_json              TEXT NOT NULL,
           state                     TEXT NOT NULL CHECK (state IN (
             'queued', 'claimed', 'launching', 'launched', 'accepted', 'processing',
             'answer-ready', 'delivering', 'retrying', 'failed', 'done'
           )),
           received_at_unix_ms       INTEGER NOT NULL,
           updated_at_unix_ms        INTEGER NOT NULL,
           claim_owner               TEXT,
           claim_expires_at_unix_ms  INTEGER,
           retry_count               INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
           retry_at_unix_ms           INTEGER,
           retry_from_state           TEXT CHECK (retry_from_state IN (
             'claimed', 'launching', 'accepted', 'processing', 'delivering'
           )),
           last_error                TEXT,
           launched_at_unix_ms       INTEGER,
           accepted_at_unix_ms       INTEGER,
           progressing_at_unix_ms    INTEGER,
           completed_at_unix_ms      INTEGER,
           observation_instance      TEXT,
           observation_session_id    TEXT,
           observation_revision      INTEGER NOT NULL DEFAULT 0 CHECK (observation_revision >= 0),
           attempt_accepted_at_unix_ms INTEGER,
           attempt_progressing_at_unix_ms INTEGER,
           latest_progress_at_unix_ms INTEGER,
           launch_expires_at_unix_ms INTEGER,
           acceptance_expires_at_unix_ms INTEGER,
           progress_expires_at_unix_ms INTEGER,
           recovery_expires_at_unix_ms INTEGER,
           absolute_work_expires_at_unix_ms INTEGER,
           recovery_count            INTEGER NOT NULL DEFAULT 0 CHECK (recovery_count >= 0),
           attempt_kind              TEXT NOT NULL DEFAULT 'ordinary'
             CHECK (attempt_kind IN ('ordinary', 'recovery')),
           pending_unavailable_notice INTEGER NOT NULL DEFAULT 0
             CHECK (pending_unavailable_notice IN (0, 1)),
           recovery_cleanup_instance  TEXT,
           recovery_cleanup_session_id TEXT,
           unavailable_notice_owner TEXT,
           unavailable_notice_expires_at_unix_ms INTEGER,
           UNIQUE (workspace_id, channel, provider_id),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL)),
           CHECK ((recovery_cleanup_instance IS NULL) =
                  (recovery_cleanup_session_id IS NULL))
         );
         CREATE INDEX IF NOT EXISTS receiver_jobs_ready
           ON receiver_jobs(state, retry_at_unix_ms, received_at_unix_ms, job_id);
         CREATE TABLE IF NOT EXISTS receiver_session_registrations (
           workspace_id          TEXT NOT NULL,
           conversation_id       TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
           agent_kind            TEXT NOT NULL CHECK (agent_kind IN ('claude', 'codex', 'opencode')),
           actor_id              TEXT NOT NULL,
           channel               TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           brain_instance_id     TEXT NOT NULL,
           registered_session_id TEXT NOT NULL,
           actual_session_id     TEXT,
           PRIMARY KEY (workspace_id, brain_instance_id)
         );",
    )?;
    if !has_launch_retry_origin(&transaction)? {
        transaction.execute_batch(
            "ALTER TABLE receiver_jobs ADD COLUMN retry_from_state TEXT
               CHECK (retry_from_state IN (
                 'claimed', 'launching', 'accepted', 'processing', 'delivering'
               ));",
        )?;
    }
    ensure_token_column(&transaction, &mut next_token)?;
    rebuild_v8_jobs_for_observations(&transaction)?;
    ensure_observation_columns(&transaction, &mut next_token)?;
    recovery::ensure_columns(&transaction)?;
    ensure_unavailable_notice_columns(&transaction)?;
    delivery::ensure_schema(&transaction)?;
    if current_version < VERSION && !had_any_recovery_column {
        recovery::migrate_v9_metadata(&transaction)?;
    }
    recovery::reconcile_metadata(&transaction)?;
    if current_version < VERSION {
        transaction.pragma_update(None, "user_version", VERSION)?;
    }
    transaction.commit()?;
    Ok(())
}

fn rebuild_v8_jobs_for_observations(connection: &Connection) -> Result<()> {
    if !has_column(connection, "job_id")? || is_v9_receiver_jobs(connection)? {
        return Ok(());
    }
    let observation_values = [
        ("launched_at_unix_ms", "NULL"),
        ("accepted_at_unix_ms", "NULL"),
        ("progressing_at_unix_ms", "NULL"),
        ("completed_at_unix_ms", "NULL"),
        ("observation_instance", "NULL"),
        ("observation_session_id", "NULL"),
        ("observation_revision", "0"),
    ]
    .into_iter()
    .map(|(column, fallback)| {
        Ok(if has_column(connection, column)? {
            column
        } else {
            fallback
        })
    })
    .collect::<Result<Vec<_>>>()?
    .join(", ");
    connection.execute_batch(&format!(
        "DROP INDEX IF EXISTS receiver_jobs_ready;
         ALTER TABLE receiver_jobs RENAME TO receiver_jobs_v8;
         CREATE TABLE receiver_jobs (
           job_id TEXT PRIMARY KEY, job_token TEXT NOT NULL UNIQUE, workspace_id TEXT NOT NULL,
           conversation_id TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
           channel TEXT NOT NULL CHECK (channel IN ('sms', 'email')), provider_id TEXT,
           inbound_json TEXT NOT NULL,
           state TEXT NOT NULL CHECK (state IN ('queued', 'claimed', 'launching', 'launched', 'accepted', 'processing', 'answer-ready', 'delivering', 'retrying', 'failed', 'done')),
           received_at_unix_ms INTEGER NOT NULL, updated_at_unix_ms INTEGER NOT NULL,
           claim_owner TEXT, claim_expires_at_unix_ms INTEGER,
           retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0), retry_at_unix_ms INTEGER,
           retry_from_state TEXT CHECK (retry_from_state IN ('claimed', 'launching', 'accepted', 'processing', 'delivering')),
           last_error TEXT, launched_at_unix_ms INTEGER, accepted_at_unix_ms INTEGER,
           progressing_at_unix_ms INTEGER, completed_at_unix_ms INTEGER,
           observation_instance TEXT, observation_session_id TEXT,
           observation_revision INTEGER NOT NULL DEFAULT 0 CHECK (observation_revision >= 0),
           UNIQUE (workspace_id, channel, provider_id),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL))
         );
         INSERT INTO receiver_jobs
           (job_id, job_token, workspace_id, conversation_id, channel, provider_id, inbound_json, state,
            received_at_unix_ms, updated_at_unix_ms, claim_owner, claim_expires_at_unix_ms,
            retry_count, retry_at_unix_ms, retry_from_state, last_error,
            launched_at_unix_ms, accepted_at_unix_ms, progressing_at_unix_ms, completed_at_unix_ms,
            observation_instance, observation_session_id, observation_revision)
         SELECT job_id, job_token, workspace_id, conversation_id, channel, provider_id, inbound_json, state,
            received_at_unix_ms, updated_at_unix_ms, claim_owner, claim_expires_at_unix_ms,
            retry_count, retry_at_unix_ms, retry_from_state, last_error,
            {observation_values} FROM receiver_jobs_v8;
         DROP TABLE receiver_jobs_v8;
         CREATE INDEX receiver_jobs_ready ON receiver_jobs(state, retry_at_unix_ms, received_at_unix_ms, job_id);"
    ))?;
    Ok(())
}

fn ensure_token_column(
    connection: &Connection,
    next_token: &mut impl FnMut() -> ReceiverJobToken,
) -> Result<()> {
    if !has_column(connection, "job_token")? && has_column(connection, "job_id")? {
        connection.execute_batch("ALTER TABLE receiver_jobs ADD COLUMN job_token TEXT;")?;
    }
    populate_job_tokens(connection, next_token)?;
    Ok(())
}

fn is_v9_receiver_jobs(connection: &Connection) -> Result<bool> {
    let sql: Option<String> = connection.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'receiver_jobs'",
        [],
        |row| row.get(0),
    )?;
    Ok(sql.is_some_and(|sql| {
        sql.contains("'launched'")
            && has_non_null_token_column(connection).unwrap_or(false)
            && [
                "job_token",
                "launched_at_unix_ms",
                "accepted_at_unix_ms",
                "progressing_at_unix_ms",
                "completed_at_unix_ms",
                "observation_instance",
                "observation_session_id",
                "observation_revision",
            ]
            .iter()
            .all(|column| sql.contains(column))
    }))
}

fn has_non_null_token_column(connection: &Connection) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT \"notnull\" FROM pragma_table_info('receiver_jobs') WHERE name = 'job_token'",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

fn ensure_observation_columns(
    connection: &Connection,
    next_token: &mut impl FnMut() -> ReceiverJobToken,
) -> Result<()> {
    for (column, definition) in [
        ("job_token", "TEXT"),
        ("launched_at_unix_ms", "INTEGER"),
        ("accepted_at_unix_ms", "INTEGER"),
        ("progressing_at_unix_ms", "INTEGER"),
        ("completed_at_unix_ms", "INTEGER"),
        ("observation_instance", "TEXT"),
        ("observation_session_id", "TEXT"),
        (
            "observation_revision",
            "INTEGER NOT NULL DEFAULT 0 CHECK (observation_revision >= 0)",
        ),
    ] {
        if !has_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_jobs ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    populate_job_tokens(connection, next_token)?;
    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS receiver_jobs_job_token ON receiver_jobs(job_token);",
    )?;
    Ok(())
}

fn has_launch_retry_origin(connection: &Connection) -> Result<bool> {
    has_column(connection, "retry_from_state")
}

pub(super) fn has_column(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_jobs')
           WHERE name = ?1
         )",
        [name],
        |row| row.get(0),
    )?)
}

pub(crate) use delivery::down_path as down_delivery_path;
#[cfg(test)]
pub(in crate::state::receiver) use delivery::down_path_with_busy_observer as down_delivery_path_with_busy_observer;
pub(crate) use downgrade::{
    down_observation_to_registration_path, down_path, down_registration_to_launch_path,
    down_to_previous_path,
};
pub(crate) use notice::down_unavailable_notice_path;
#[cfg(test)]
pub(super) use notice::down_unavailable_notice_path_with_busy_observer;
pub(crate) use recovery::down_cleanup_fence_path;
pub(crate) use recovery::down_to_observation_path as down_recovery_to_observation_path;
#[cfg(test)]
pub(in crate::state::receiver) use recovery::{
    down_cleanup_fence_path_with_busy_observer, down_to_observation_path_with_busy_observer,
};

use notice::ensure_unavailable_notice_columns;
