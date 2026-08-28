use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension as _};

mod downgrade;

pub(crate) use downgrade::down_path;
#[cfg(test)]
pub(in crate::state::receiver) use downgrade::down_path_with_busy_observer;

const PROVIDER_REFERENCE_CONSTRAINT: &str = "length(trim(provider_reference)) > 0";

const CREATE_DELIVERY_TABLE: &str = "CREATE TABLE IF NOT EXISTS receiver_deliveries (
           delivery_id                 TEXT PRIMARY KEY,
           job_id                      TEXT NOT NULL REFERENCES receiver_jobs(job_id) ON DELETE CASCADE,
           job_token                   TEXT NOT NULL,
           response_kind               TEXT NOT NULL CHECK (response_kind IN (
             'final-answer', 'unavailable-notice', 'control-acknowledgement', 'fallback-notice'
           )),
           envelope_json               TEXT NOT NULL,
           completion_evidence_json    TEXT,
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
           CHECK (state != 'acknowledged' OR (
             provider_reference IS NOT NULL AND length(trim(provider_reference)) > 0
           )),
           CHECK (state != 'ambiguous' OR ambiguity_reason IS NOT NULL)
         );";

const CREATE_ANSWER_CLEANUP_TABLE: &str = "CREATE TABLE IF NOT EXISTS receiver_answer_cleanups (
           job_id                  TEXT PRIMARY KEY REFERENCES receiver_jobs(job_id) ON DELETE CASCADE,
           job_token               TEXT NOT NULL,
           workspace_id            TEXT NOT NULL,
           conversation_id         TEXT NOT NULL,
           brain_instance_id       TEXT NOT NULL,
           agent_kind              TEXT NOT NULL CHECK (agent_kind IN ('claude', 'codex', 'opencode')),
           actor_id                TEXT NOT NULL,
           channel                 TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           registered_session_id   TEXT NOT NULL,
           actual_session_id       TEXT NOT NULL,
           controller_shutdown_acknowledged INTEGER NOT NULL DEFAULT 0
             CHECK (controller_shutdown_acknowledged IN (0, 1)),
           session_released        INTEGER NOT NULL DEFAULT 0 CHECK (session_released IN (0, 1)),
           artifacts_removed       INTEGER NOT NULL DEFAULT 0 CHECK (artifacts_removed IN (0, 1)),
           created_at_unix_ms      INTEGER NOT NULL,
           updated_at_unix_ms      INTEGER NOT NULL
         );";

pub(super) fn ensure_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(CREATE_DELIVERY_TABLE)?;
    connection.execute_batch(CREATE_ANSWER_CLEANUP_TABLE)?;
    ensure_optional_columns(connection)?;
    ensure_answer_cleanup_optional_columns(connection)?;
    ensure_answer_cleanup_columns(connection)?;
    ensure_answer_cleanup_table_contract(connection)?;
    reconcile_rows(connection)?;
    ensure_table_contract(connection)?;
    ensure_managed_indexes(connection)?;
    Ok(())
}

fn ensure_answer_cleanup_table_contract(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT name FROM pragma_index_list('receiver_answer_cleanups')
         WHERE \"unique\" = 1",
    )?;
    let indexes = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let has_legacy_instance_unique = indexes.into_iter().try_fold(false, |found, index| {
        if found {
            return Ok::<bool, anyhow::Error>(true);
        }
        let mut columns =
            connection.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = columns
            .query_map([index], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(columns == ["workspace_id", "brain_instance_id"])
    })?;
    if !has_legacy_instance_unique {
        return Ok(());
    }
    connection.execute_batch(
        "ALTER TABLE receiver_answer_cleanups
           RENAME TO receiver_answer_cleanups_v12_rebuild;",
    )?;
    connection.execute_batch(CREATE_ANSWER_CLEANUP_TABLE)?;
    connection.execute_batch(
        "INSERT INTO receiver_answer_cleanups
           (job_id, job_token, workspace_id, conversation_id, brain_instance_id,
            agent_kind, actor_id, channel, registered_session_id, actual_session_id,
            controller_shutdown_acknowledged, session_released, artifacts_removed,
            created_at_unix_ms, updated_at_unix_ms)
         SELECT job_id, job_token, workspace_id, conversation_id, brain_instance_id,
                agent_kind, actor_id, channel, registered_session_id, actual_session_id,
                CASE WHEN session_released = 1 THEN 1
                     ELSE controller_shutdown_acknowledged END,
                session_released, artifacts_removed,
                created_at_unix_ms, updated_at_unix_ms
         FROM receiver_answer_cleanups_v12_rebuild;
         DROP TABLE receiver_answer_cleanups_v12_rebuild;",
    )?;
    Ok(())
}

fn ensure_answer_cleanup_columns(connection: &Connection) -> Result<()> {
    for required in [
        "job_id",
        "job_token",
        "workspace_id",
        "conversation_id",
        "brain_instance_id",
        "agent_kind",
        "actor_id",
        "channel",
        "registered_session_id",
        "actual_session_id",
        "controller_shutdown_acknowledged",
        "session_released",
        "artifacts_removed",
        "created_at_unix_ms",
        "updated_at_unix_ms",
    ] {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM pragma_table_info('receiver_answer_cleanups') WHERE name = ?1
             )",
            [required],
            |row| row.get(0),
        )?;
        if !exists {
            bail!("receiver answer cleanup schema is missing required column {required}");
        }
    }
    Ok(())
}

fn ensure_answer_cleanup_optional_columns(connection: &Connection) -> Result<()> {
    let acknowledged_exists: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_answer_cleanups')
           WHERE name = 'controller_shutdown_acknowledged'
         )",
        [],
        |row| row.get(0),
    )?;
    if !acknowledged_exists {
        connection.execute_batch(
            "ALTER TABLE receiver_answer_cleanups
             ADD COLUMN controller_shutdown_acknowledged INTEGER NOT NULL DEFAULT 0
               CHECK (controller_shutdown_acknowledged IN (0, 1));",
        )?;
    }
    connection.execute(
        "UPDATE receiver_answer_cleanups
         SET controller_shutdown_acknowledged = 1
         WHERE session_released = 1 AND controller_shutdown_acknowledged = 0",
        [],
    )?;
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
        ("completion_evidence_json", "TEXT"),
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
         SET state = 'ambiguous', provider_reference = NULL,
             ambiguity_reason = 'provider-acknowledgement-malformed'
         WHERE state = 'acknowledged'
           AND (provider_reference IS NULL OR length(trim(provider_reference)) = 0);
         UPDATE receiver_deliveries
         SET ambiguity_reason = 'result-commit-unknown'
         WHERE state = 'ambiguous' AND ambiguity_reason IS NULL;",
    )?;
    Ok(())
}

fn ensure_table_contract(connection: &Connection) -> Result<()> {
    let sql: String = connection.query_row(
        "SELECT sql FROM sqlite_master
         WHERE type = 'table' AND name = 'receiver_deliveries'",
        [],
        |row| row.get(0),
    )?;
    if sql.contains(PROVIDER_REFERENCE_CONSTRAINT) {
        return Ok(());
    }
    reject_duplicate_semantic_responses(connection)?;
    connection.execute_batch(
        "ALTER TABLE receiver_deliveries RENAME TO receiver_deliveries_v12_rebuild;",
    )?;
    connection.execute_batch(CREATE_DELIVERY_TABLE)?;
    connection.execute_batch(
        "INSERT INTO receiver_deliveries
           (delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, state,
            attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, first_attempt_at_unix_ms, provider_reference,
            error_category, ambiguity_reason, created_at_unix_ms, updated_at_unix_ms)
         SELECT delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, state,
            attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, first_attempt_at_unix_ms, provider_reference,
            error_category, ambiguity_reason, created_at_unix_ms, updated_at_unix_ms
         FROM receiver_deliveries_v12_rebuild;
         DROP TABLE receiver_deliveries_v12_rebuild;",
    )?;
    Ok(())
}

fn ensure_managed_indexes(connection: &Connection) -> Result<()> {
    if !index_matches(
        connection,
        "receiver_deliveries_job_kind",
        true,
        &["job_id", "response_kind"],
    )? {
        reject_duplicate_semantic_responses(connection)?;
        connection.execute_batch(
            "DROP INDEX IF EXISTS receiver_deliveries_job_kind;
             CREATE UNIQUE INDEX receiver_deliveries_job_kind
               ON receiver_deliveries(job_id, response_kind);",
        )?;
    }
    if !index_matches(
        connection,
        "receiver_deliveries_due",
        false,
        &[
            "state",
            "retry_at_unix_ms",
            "created_at_unix_ms",
            "delivery_id",
        ],
    )? {
        connection.execute_batch(
            "DROP INDEX IF EXISTS receiver_deliveries_due;
             CREATE INDEX receiver_deliveries_due
               ON receiver_deliveries(
                 state, retry_at_unix_ms, created_at_unix_ms, delivery_id
               );",
        )?;
    }
    Ok(())
}

fn index_matches(
    connection: &Connection,
    index_name: &str,
    expected_unique: bool,
    expected_columns: &[&str],
) -> Result<bool> {
    let unique = connection
        .query_row(
            "SELECT \"unique\" FROM pragma_index_list('receiver_deliveries')
             WHERE name = ?1",
            [index_name],
            |row| row.get::<_, bool>(0),
        )
        .optional()?;
    let Some(unique) = unique else {
        return Ok(false);
    };
    let mut statement =
        connection.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
    let columns = statement
        .query_map([index_name], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(unique == expected_unique
        && columns
            .iter()
            .map(String::as_str)
            .eq(expected_columns.iter().copied()))
}

fn reject_duplicate_semantic_responses(connection: &Connection) -> Result<()> {
    let has_duplicates: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM receiver_deliveries
           GROUP BY job_id, response_kind HAVING COUNT(*) > 1
         )",
        [],
        |row| row.get(0),
    )?;
    if has_duplicates {
        bail!("receiver delivery schema contains duplicate semantic responses");
    }
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
