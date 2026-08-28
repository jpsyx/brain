use anyhow::{Result, bail};
use rusqlite::Connection;

const CURRENT_CONTRACT: &str =
    "state NOT IN ('failed', 'ambiguous') OR fallback_decision IS NOT NULL";

const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS receiver_deliveries (
           delivery_id                 TEXT PRIMARY KEY,
           job_id                      TEXT NOT NULL REFERENCES receiver_jobs(job_id) ON DELETE CASCADE,
           job_token                   TEXT NOT NULL,
           response_kind               TEXT NOT NULL CHECK (response_kind IN (
             'final-answer', 'unavailable-notice', 'control-acknowledgement', 'fallback-notice'
           )),
           envelope_json               TEXT NOT NULL,
           completion_evidence_json    TEXT,
           frozen_fallbacks_json       TEXT NOT NULL DEFAULT '[]',
           state                       TEXT NOT NULL CHECK (state IN (
             'ready', 'delivering', 'retrying', 'acknowledged', 'failed', 'ambiguous'
           )),
           attempt_id                  TEXT,
           attempt_count               INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
           retry_at_unix_ms             INTEGER,
           claim_owner                 TEXT,
           claim_expires_at_unix_ms    INTEGER,
           provider_io_started         INTEGER NOT NULL DEFAULT 0
             CHECK (provider_io_started IN (0, 1)),
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
           fallback_decision           TEXT CHECK (fallback_decision IN (
             'fallback-planned', 'no-safe-fallback'
           )),
           created_at_unix_ms          INTEGER NOT NULL,
           updated_at_unix_ms          INTEGER NOT NULL,
           UNIQUE (job_id, response_kind),
           CHECK ((claim_owner IS NULL) = (claim_expires_at_unix_ms IS NULL)),
           CHECK (state = 'delivering' OR claim_owner IS NULL),
           CHECK (state != 'delivering' OR (
             attempt_id IS NOT NULL AND claim_owner IS NOT NULL
           )),
           CHECK (state = 'delivering' OR provider_io_started = 0),
           CHECK (provider_io_started = 0 OR first_attempt_at_unix_ms IS NOT NULL),
           CHECK (state = 'retrying' OR retry_at_unix_ms IS NULL),
           CHECK (state != 'retrying' OR retry_at_unix_ms IS NOT NULL),
           CHECK (state != 'acknowledged' OR (
             provider_reference IS NOT NULL AND length(trim(provider_reference)) > 0
           )),
           CHECK (state != 'ambiguous' OR ambiguity_reason IS NOT NULL),
           CHECK (state NOT IN ('failed', 'ambiguous') OR fallback_decision IS NOT NULL)
         );";

pub(super) fn create_table(connection: &Connection) -> Result<()> {
    connection.execute_batch(CREATE_TABLE)?;
    Ok(())
}

pub(super) fn ensure_optional_columns(connection: &Connection) -> Result<()> {
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
        if !has_column(connection, required)? {
            bail!("receiver delivery schema is missing required column {required}");
        }
    }
    for (column, definition) in [
        ("attempt_id", "TEXT"),
        ("completion_evidence_json", "TEXT"),
        ("frozen_fallbacks_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("retry_at_unix_ms", "INTEGER"),
        ("claim_owner", "TEXT"),
        ("claim_expires_at_unix_ms", "INTEGER"),
        ("provider_io_started", "INTEGER NOT NULL DEFAULT 0"),
        ("first_attempt_at_unix_ms", "INTEGER"),
        ("provider_reference", "TEXT"),
        ("error_category", "TEXT"),
        ("ambiguity_reason", "TEXT"),
        ("fallback_decision", "TEXT"),
    ] {
        if !has_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_deliveries ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    Ok(())
}

pub(super) fn ensure_table_contract(connection: &Connection) -> Result<()> {
    let sql: String = connection.query_row(
        "SELECT sql FROM sqlite_master
         WHERE type = 'table' AND name = 'receiver_deliveries'",
        [],
        |row| row.get(0),
    )?;
    if sql.contains(CURRENT_CONTRACT) {
        return Ok(());
    }
    super::indexes::reject_duplicate_semantic_responses(connection)?;
    connection.execute_batch(
        "ALTER TABLE receiver_deliveries RENAME TO receiver_deliveries_v12_rebuild;",
    )?;
    connection.execute_batch(CREATE_TABLE)?;
    connection.execute_batch(
        "INSERT INTO receiver_deliveries
           (delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, frozen_fallbacks_json, state,
            attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, provider_io_started, first_attempt_at_unix_ms, provider_reference,
            error_category, ambiguity_reason, fallback_decision,
            created_at_unix_ms, updated_at_unix_ms)
         SELECT delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, frozen_fallbacks_json, state,
            attempt_id, attempt_count, retry_at_unix_ms, claim_owner,
            claim_expires_at_unix_ms, provider_io_started, first_attempt_at_unix_ms, provider_reference,
            error_category, ambiguity_reason, fallback_decision,
            created_at_unix_ms, updated_at_unix_ms
         FROM receiver_deliveries_v12_rebuild;
         DROP TABLE receiver_deliveries_v12_rebuild;",
    )?;
    Ok(())
}

fn has_column(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_deliveries') WHERE name = ?1
         )",
        [name],
        |row| row.get(0),
    )?)
}
