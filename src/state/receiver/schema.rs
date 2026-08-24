use anyhow::Result;
use rusqlite::Connection;

pub(super) const VERSION: i32 = 8;
const LAUNCH_VERSION: i32 = 7;

pub(in crate::state) fn up(connection: &Connection, current_version: i32) -> Result<()> {
    let transaction = connection.unchecked_transaction()?;
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
           workspace_id              TEXT NOT NULL,
           conversation_id           TEXT NOT NULL REFERENCES receiver_conversations(conversation_id),
           channel                   TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           provider_id               TEXT,
           inbound_json              TEXT NOT NULL,
           state                     TEXT NOT NULL CHECK (state IN (
             'queued', 'claimed', 'launching', 'accepted', 'processing',
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
    if current_version < VERSION {
        transaction.pragma_update(None, "user_version", VERSION)?;
    }
    transaction.commit()?;
    Ok(())
}

fn has_launch_retry_origin(connection: &Connection) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_jobs')
           WHERE name = 'retry_from_state'
         )",
        [],
        |row| row.get(0),
    )?)
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
    if version != VERSION {
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
    if matches!(version, VERSION | LAUNCH_VERSION | 6) {
        transaction.pragma_update(None, "user_version", 5)?;
    }
    transaction.commit()?;
    Ok(())
}
