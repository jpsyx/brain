use anyhow::Result;
use rusqlite::Connection;

use super::{OBSERVATION_VERSION, RECOVERY_VERSION, has_column};
use crate::state::Db;

mod cleanup;

const COLUMNS: &[(&str, &str)] = &[
    ("attempt_accepted_at_unix_ms", "INTEGER"),
    ("attempt_progressing_at_unix_ms", "INTEGER"),
    ("latest_progress_at_unix_ms", "INTEGER"),
    ("launch_expires_at_unix_ms", "INTEGER"),
    ("acceptance_expires_at_unix_ms", "INTEGER"),
    ("progress_expires_at_unix_ms", "INTEGER"),
    ("recovery_expires_at_unix_ms", "INTEGER"),
    ("absolute_work_expires_at_unix_ms", "INTEGER"),
    (
        "recovery_count",
        "INTEGER NOT NULL DEFAULT 0 CHECK (recovery_count >= 0)",
    ),
    (
        "attempt_kind",
        "TEXT NOT NULL DEFAULT 'ordinary' CHECK (attempt_kind IN ('ordinary', 'recovery'))",
    ),
    (
        "pending_unavailable_notice",
        "INTEGER NOT NULL DEFAULT 0 CHECK (pending_unavailable_notice IN (0, 1))",
    ),
    ("recovery_cleanup_instance", "TEXT"),
    ("recovery_cleanup_session_id", "TEXT"),
];

pub(super) fn had_any_column(connection: &Connection) -> bool {
    COLUMNS
        .iter()
        .any(|(column, _)| has_column(connection, column).unwrap_or(false))
}

pub(super) fn ensure_columns(connection: &Connection) -> Result<()> {
    for (column, definition) in COLUMNS {
        if !has_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_jobs ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    Ok(())
}

pub(super) fn migrate_v9_metadata(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET attempt_accepted_at_unix_ms = accepted_at_unix_ms,
             attempt_progressing_at_unix_ms = progressing_at_unix_ms,
             latest_progress_at_unix_ms = progressing_at_unix_ms,
             launch_expires_at_unix_ms = CASE
               WHEN state IN ('claimed', 'launching') THEN 0
               ELSE NULL
             END,
             acceptance_expires_at_unix_ms = CASE
               WHEN state = 'launched' THEN 0
               ELSE NULL
             END,
             progress_expires_at_unix_ms = CASE
               WHEN state = 'accepted' THEN
                 CASE
                   WHEN accepted_at_unix_ms IS NULL OR updated_at_unix_ms < 0 THEN 0
                   WHEN MIN(accepted_at_unix_ms, updated_at_unix_ms) > 9223372036854475807
                     THEN 9223372036854775807
                   ELSE MIN(accepted_at_unix_ms, updated_at_unix_ms) + 300000
                 END
               WHEN state = 'processing' THEN
                 CASE
                   WHEN progressing_at_unix_ms IS NULL OR updated_at_unix_ms < 0 THEN 0
                   WHEN MIN(progressing_at_unix_ms, updated_at_unix_ms) > 9223372036854475807
                     THEN 9223372036854775807
                   ELSE MIN(progressing_at_unix_ms, updated_at_unix_ms) + 300000
                 END
               ELSE NULL
             END,
             recovery_expires_at_unix_ms = NULL,
             absolute_work_expires_at_unix_ms = CASE
               WHEN state IN ('accepted', 'processing') THEN
                 CASE
                   WHEN accepted_at_unix_ms IS NULL OR updated_at_unix_ms < 0 THEN 0
                   WHEN MIN(accepted_at_unix_ms, updated_at_unix_ms) > 9223372036852975807
                     THEN 9223372036854775807
                   ELSE MIN(accepted_at_unix_ms, updated_at_unix_ms) + 1800000
                 END
               ELSE NULL
             END,
             recovery_count = 0,
             attempt_kind = 'ordinary',
             pending_unavailable_notice = 0,
             recovery_cleanup_instance = NULL,
             recovery_cleanup_session_id = NULL;",
    )?;
    Ok(())
}

pub(super) fn reconcile_metadata(connection: &Connection) -> Result<()> {
    cleanup::reconcile_partial_fences(connection)?;
    let needs_repair = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM receiver_jobs
           WHERE (attempt_kind = 'recovery' AND recovery_count = 0)
              OR (attempt_kind = 'ordinary' AND recovery_count > 0)
              OR (attempt_kind = 'ordinary' AND accepted_at_unix_ms IS NOT NULL
                  AND attempt_accepted_at_unix_ms IS NULL)
              OR (attempt_kind = 'ordinary' AND progressing_at_unix_ms IS NOT NULL
                  AND attempt_progressing_at_unix_ms IS NULL)
              OR (progressing_at_unix_ms IS NOT NULL
                  AND latest_progress_at_unix_ms IS NULL)
              OR (state IN ('claimed', 'launching')
                  AND launch_expires_at_unix_ms IS NULL)
              OR (state = 'launched' AND acceptance_expires_at_unix_ms IS NULL)
              OR (state IN ('accepted', 'processing')
                  AND progress_expires_at_unix_ms IS NULL)
              OR (attempt_kind = 'recovery' AND state NOT IN ('failed', 'done')
                  AND recovery_expires_at_unix_ms IS NULL)
              OR (state IN ('accepted', 'processing')
                  AND absolute_work_expires_at_unix_ms IS NULL)
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !needs_repair {
        return Ok(());
    }
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET recovery_count = CASE
               WHEN attempt_kind = 'recovery' AND recovery_count = 0 THEN 1
               ELSE recovery_count
             END;
         UPDATE receiver_jobs
         SET attempt_kind = CASE
               WHEN recovery_count > 0 THEN 'recovery' ELSE attempt_kind
             END,
             attempt_accepted_at_unix_ms = CASE
               WHEN attempt_kind = 'ordinary'
                 THEN COALESCE(attempt_accepted_at_unix_ms, accepted_at_unix_ms)
               ELSE attempt_accepted_at_unix_ms
             END,
             attempt_progressing_at_unix_ms = CASE
               WHEN attempt_kind = 'ordinary'
                 THEN COALESCE(attempt_progressing_at_unix_ms, progressing_at_unix_ms)
               ELSE attempt_progressing_at_unix_ms
             END,
             latest_progress_at_unix_ms = COALESCE(
               latest_progress_at_unix_ms, progressing_at_unix_ms
             ),
             launch_expires_at_unix_ms = CASE
               WHEN state IN ('claimed', 'launching')
                 THEN COALESCE(launch_expires_at_unix_ms, 0)
               ELSE launch_expires_at_unix_ms
             END,
             acceptance_expires_at_unix_ms = CASE
               WHEN state = 'launched'
                 THEN COALESCE(acceptance_expires_at_unix_ms, 0)
               ELSE acceptance_expires_at_unix_ms
             END,
             progress_expires_at_unix_ms = CASE
               WHEN state IN ('accepted', 'processing')
                 THEN COALESCE(progress_expires_at_unix_ms, 0)
               ELSE progress_expires_at_unix_ms
             END,
             recovery_expires_at_unix_ms = CASE
               WHEN attempt_kind = 'recovery' AND state NOT IN ('failed', 'done')
                 THEN COALESCE(recovery_expires_at_unix_ms, 0)
               ELSE recovery_expires_at_unix_ms
             END,
             absolute_work_expires_at_unix_ms = CASE
               WHEN state IN ('accepted', 'processing')
                 THEN COALESCE(absolute_work_expires_at_unix_ms, 0)
               ELSE absolute_work_expires_at_unix_ms
             END;",
    )?;
    Ok(())
}

pub(crate) fn down_cleanup_fence_path(path: &std::path::Path) -> Result<()> {
    down_cleanup_fence_path_inner(path, None)
}

#[cfg(test)]
pub(in crate::state::receiver) fn down_cleanup_fence_path_with_busy_observer(
    path: &std::path::Path,
    observer: fn(i32) -> bool,
) -> Result<()> {
    down_cleanup_fence_path_inner(path, Some(observer))
}

fn down_cleanup_fence_path_inner(
    path: &std::path::Path,
    busy_observer: Option<fn(i32) -> bool>,
) -> Result<()> {
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
    let has_instance = has_column(&transaction, "recovery_cleanup_instance")?;
    let has_session = has_column(&transaction, "recovery_cleanup_session_id")?;
    if !has_instance || !has_session {
        transaction.commit()?;
        return Ok(());
    }
    transaction.execute_batch(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = CASE
               WHEN state = 'failed' AND last_error LIKE 'recovery-%' THEN last_error
               ELSE 'recovery-native-session-unavailable'
             END,
             observation_instance = NULL, observation_session_id = NULL,
             observation_revision = 0, attempt_accepted_at_unix_ms = NULL,
             attempt_progressing_at_unix_ms = NULL,
             latest_progress_at_unix_ms = NULL,
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             recovery_count = MAX(recovery_count, 1), attempt_kind = 'recovery',
             pending_unavailable_notice = 1,
             recovery_cleanup_instance = CASE
               WHEN recovery_cleanup_instance IS NOT NULL
                AND recovery_cleanup_session_id IS NOT NULL
               THEN recovery_cleanup_instance ELSE NULL END,
             recovery_cleanup_session_id = CASE
               WHEN recovery_cleanup_instance IS NOT NULL
                AND recovery_cleanup_session_id IS NOT NULL
               THEN recovery_cleanup_session_id ELSE NULL END
         WHERE recovery_cleanup_instance IS NOT NULL
            OR recovery_cleanup_session_id IS NOT NULL;",
    )?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn down_to_observation_path(path: &std::path::Path) -> Result<()> {
    down_to_observation_path_inner(path, None)
}

#[cfg(test)]
pub(in crate::state::receiver) fn down_to_observation_path_with_busy_observer(
    path: &std::path::Path,
    observer: fn(i32) -> bool,
) -> Result<()> {
    down_to_observation_path_inner(path, Some(observer))
}

fn down_to_observation_path_inner(
    path: &std::path::Path,
    busy_observer: Option<fn(i32) -> bool>,
) -> Result<()> {
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
    if version != RECOVERY_VERSION {
        transaction.commit()?;
        return Ok(());
    }
    transaction.execute_batch(
        "DROP INDEX IF EXISTS receiver_jobs_ready;
         DROP INDEX IF EXISTS receiver_jobs_job_token;
         ALTER TABLE receiver_jobs RENAME TO receiver_jobs_v10;
         CREATE TABLE receiver_jobs (
           job_id TEXT PRIMARY KEY, job_token TEXT NOT NULL UNIQUE, workspace_id TEXT NOT NULL,
           conversation_id TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
           channel TEXT NOT NULL CHECK (channel IN ('sms', 'email')), provider_id TEXT,
           inbound_json TEXT NOT NULL,
           state TEXT NOT NULL CHECK (state IN (
             'queued', 'claimed', 'launching', 'launched', 'accepted', 'processing',
             'answer-ready', 'delivering', 'retrying', 'failed', 'done'
           )),
           received_at_unix_ms INTEGER NOT NULL, updated_at_unix_ms INTEGER NOT NULL,
           claim_owner TEXT, claim_expires_at_unix_ms INTEGER,
           retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
           retry_at_unix_ms INTEGER,
           retry_from_state TEXT CHECK (retry_from_state IN (
             'claimed', 'launching', 'accepted', 'processing', 'delivering'
           )),
           last_error TEXT, launched_at_unix_ms INTEGER, accepted_at_unix_ms INTEGER,
           progressing_at_unix_ms INTEGER, completed_at_unix_ms INTEGER,
           observation_instance TEXT, observation_session_id TEXT,
           observation_revision INTEGER NOT NULL DEFAULT 0 CHECK (observation_revision >= 0),
           UNIQUE (workspace_id, channel, provider_id),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL))
         );
         INSERT INTO receiver_jobs
           (job_id, job_token, workspace_id, conversation_id, channel, provider_id, inbound_json,
            state, received_at_unix_ms, updated_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, retry_count, retry_at_unix_ms, retry_from_state,
            last_error, launched_at_unix_ms, accepted_at_unix_ms, progressing_at_unix_ms,
            completed_at_unix_ms, observation_instance, observation_session_id,
            observation_revision)
         SELECT job_id, job_token, workspace_id, conversation_id, channel, provider_id,
            inbound_json,
            CASE
              WHEN state NOT IN ('failed', 'done')
                AND (attempt_kind = 'recovery' OR recovery_count > 0)
              THEN 'failed' ELSE state END,
            received_at_unix_ms, updated_at_unix_ms,
            CASE
              WHEN state NOT IN ('failed', 'done')
                AND (attempt_kind = 'recovery' OR recovery_count > 0)
              THEN NULL ELSE claim_owner END,
            CASE
              WHEN state NOT IN ('failed', 'done')
                AND (attempt_kind = 'recovery' OR recovery_count > 0)
              THEN NULL ELSE claim_expires_at_unix_ms END,
            retry_count,
            CASE
              WHEN state NOT IN ('failed', 'done')
                AND (attempt_kind = 'recovery' OR recovery_count > 0)
              THEN NULL ELSE retry_at_unix_ms END,
            CASE
              WHEN state NOT IN ('failed', 'done')
                AND (attempt_kind = 'recovery' OR recovery_count > 0)
              THEN NULL ELSE retry_from_state END,
            CASE
              WHEN state NOT IN ('failed', 'done')
                AND (attempt_kind = 'recovery' OR recovery_count > 0)
              THEN 'downgrade-no-replay' ELSE last_error END,
            launched_at_unix_ms, accepted_at_unix_ms, progressing_at_unix_ms,
            completed_at_unix_ms, observation_instance, observation_session_id,
            observation_revision
         FROM receiver_jobs_v10;
         DROP TABLE receiver_jobs_v10;
         CREATE INDEX receiver_jobs_ready
           ON receiver_jobs(state, retry_at_unix_ms, received_at_unix_ms, job_id);",
    )?;
    transaction.pragma_update(None, "user_version", OBSERVATION_VERSION)?;
    transaction.commit()?;
    Ok(())
}
