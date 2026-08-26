use anyhow::Result;
use rusqlite::{Connection, OptionalExtension as _};

use super::ReceiverJobToken;

mod token;
use token::populate_job_tokens;

pub(super) const VERSION: i32 = 10;
const OBSERVATION_VERSION: i32 = 9;
const REGISTRATION_VERSION: i32 = 8;
const LAUNCH_VERSION: i32 = 7;

pub(in crate::state) fn up(connection: &Connection, current_version: i32) -> Result<()> {
    up_with_token_factory(connection, current_version, ReceiverJobToken::new)
}

pub(super) fn up_with_token_factory(
    connection: &Connection,
    current_version: i32,
    mut next_token: impl FnMut() -> ReceiverJobToken,
) -> Result<()> {
    let transaction = connection.unchecked_transaction()?;
    let had_any_recovery_column = [
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
    ]
    .iter()
    .any(|column| has_column(&transaction, column).unwrap_or(false));
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
           UNIQUE (workspace_id, channel, provider_id),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL))
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
    ensure_recovery_columns(&transaction)?;
    if current_version < VERSION && !had_any_recovery_column {
        migrate_v9_recovery_metadata(&transaction)?;
    }
    reconcile_recovery_metadata(&transaction)?;
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

fn ensure_recovery_columns(connection: &Connection) -> Result<()> {
    for (column, definition) in [
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
    ] {
        if !has_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_jobs ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    Ok(())
}

fn migrate_v9_recovery_metadata(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET attempt_accepted_at_unix_ms = accepted_at_unix_ms,
             attempt_progressing_at_unix_ms = progressing_at_unix_ms,
             latest_progress_at_unix_ms = progressing_at_unix_ms,
             launch_expires_at_unix_ms = CASE
               WHEN state IN ('claimed', 'launching') THEN
                 CASE
                   WHEN updated_at_unix_ms < 0 THEN 0
                   WHEN updated_at_unix_ms > 9223372036854655807
                     THEN 9223372036854775807
                   ELSE updated_at_unix_ms + 120000
                 END
               ELSE NULL
             END,
             acceptance_expires_at_unix_ms = CASE
               WHEN state = 'launched' THEN
                 CASE
                   WHEN launched_at_unix_ms IS NULL OR launched_at_unix_ms < 0 THEN 0
                   WHEN launched_at_unix_ms > 9223372036854685807
                     THEN 9223372036854775807
                   ELSE launched_at_unix_ms + 90000
                 END
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
             pending_unavailable_notice = 0;",
    )?;
    Ok(())
}

fn reconcile_recovery_metadata(connection: &Connection) -> Result<()> {
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

fn has_launch_retry_origin(connection: &Connection) -> Result<bool> {
    has_column(connection, "retry_from_state")
}

fn has_column(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_jobs')
           WHERE name = ?1
         )",
        [name],
        |row| row.get(0),
    )?)
}

pub(crate) fn down_recovery_to_observation_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != VERSION {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
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
