use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension as _};

mod cleanup_schema;
mod downgrade;

pub(crate) use downgrade::down_path;
#[cfg(test)]
pub(in crate::state::receiver) use downgrade::down_path_with_busy_observer;

const CURRENT_DELIVERY_CONTRACT: &str =
    "state NOT IN ('failed', 'ambiguous') OR fallback_decision IS NOT NULL";

const CREATE_DELIVERY_TABLE: &str = "CREATE TABLE IF NOT EXISTS receiver_deliveries (
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

pub(super) fn ensure_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(CREATE_DELIVERY_TABLE)?;
    cleanup_schema::create_table(connection)?;
    ensure_optional_columns(connection)?;
    cleanup_schema::ensure_optional_columns(connection)?;
    cleanup_schema::ensure_columns(connection)?;
    cleanup_schema::ensure_table_contract(connection)?;
    reconcile_rows(connection)?;
    ensure_table_contract(connection)?;
    migrate_legacy_pending_notices(connection)?;
    ensure_managed_indexes(connection)?;
    Ok(())
}

fn migrate_legacy_pending_notices(connection: &Connection) -> Result<()> {
    let pending = {
        let mut statement = connection.prepare(
            "SELECT job_id, job_token, inbound_json, response_sender, updated_at_unix_ms
             FROM receiver_jobs
             WHERE state = 'failed' AND pending_unavailable_notice = 1
               AND recovery_cleanup_instance IS NULL
               AND recovery_cleanup_session_id IS NULL
             ORDER BY received_at_unix_ms, job_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (job_id, token, inbound_json, response_sender, observed) in pending {
        let job_id = crate::state::ReceiverJobId::parse(&job_id)?;
        let token = crate::state::ReceiverJobToken::parse(&token)?;
        let inbound = super::super::store::decode_inbound(&inbound_json, response_sender)?;
        let notice = crate::server::reply::unanswered_notice(
            super::super::store::response_intent::channel_label(inbound.channel),
        );
        let inserted = super::super::store::response_intent::insert(
            connection,
            job_id,
            token,
            &inbound,
            crate::state::ReceiverResponseKind::UnavailableNotice,
            &notice.text,
            observed,
        );
        match inserted {
            Ok(_) => {
                connection.execute(
                    "UPDATE receiver_jobs
                     SET state = 'answer-ready', pending_unavailable_notice = 0,
                         claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                         retry_at_unix_ms = NULL, retry_from_state = NULL
                     WHERE job_id = ?1 AND job_token = ?2 AND state = 'failed'
                       AND pending_unavailable_notice = 1
                       AND EXISTS (SELECT 1 FROM receiver_deliveries
                         WHERE job_id = ?1 AND job_token = ?2
                           AND response_kind = 'unavailable-notice')",
                    rusqlite::params![job_id.to_string(), token.to_string()],
                )?;
            }
            Err(error)
                if error
                    .downcast_ref::<crate::state::ReceiverDeliveryRenderError>()
                    .is_some() =>
            {
                connection.execute(
                    "UPDATE receiver_jobs
                     SET pending_unavailable_notice = 0,
                         last_error = 'notice-no-authorized-destination'
                     WHERE job_id = ?1 AND job_token = ?2 AND state = 'failed'
                       AND pending_unavailable_notice = 1",
                    rusqlite::params![job_id.to_string(), token.to_string()],
                )?;
            }
            Err(error) => return Err(error),
        }
    }
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
        if !has_delivery_column(connection, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE receiver_deliveries ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    Ok(())
}

fn reconcile_rows(connection: &Connection) -> Result<()> {
    normalize_frozen_fallbacks(connection)?;
    connection.execute_batch(
        "UPDATE receiver_deliveries
         SET state = 'ambiguous', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, ambiguity_reason = 'result-commit-unknown',
             error_category = NULL, provider_io_started = 0,
             fallback_decision = COALESCE(fallback_decision, 'no-safe-fallback')
         WHERE (claim_owner IS NULL) != (claim_expires_at_unix_ms IS NULL)
            OR (state = 'delivering' AND (
              attempt_id IS NULL OR claim_owner IS NULL
            ));
         UPDATE receiver_deliveries
         SET claim_owner = NULL, claim_expires_at_unix_ms = NULL
             , provider_io_started = 0
         WHERE state != 'delivering';
         UPDATE receiver_deliveries
         SET retry_at_unix_ms = NULL
         WHERE state != 'retrying';
         UPDATE receiver_deliveries
         SET state = 'failed', error_category = 'invalid-request',
             fallback_decision = COALESCE(fallback_decision, 'no-safe-fallback')
         WHERE state = 'retrying' AND retry_at_unix_ms IS NULL;
         UPDATE receiver_deliveries
         SET state = 'ambiguous', provider_reference = NULL,
             ambiguity_reason = 'provider-acknowledgement-malformed',
             fallback_decision = COALESCE(fallback_decision, 'no-safe-fallback')
         WHERE state = 'acknowledged'
           AND (provider_reference IS NULL OR length(trim(provider_reference)) = 0);
         UPDATE receiver_deliveries
         SET ambiguity_reason = 'result-commit-unknown'
         WHERE state = 'ambiguous' AND ambiguity_reason IS NULL;
         UPDATE receiver_deliveries
         SET fallback_decision = 'no-safe-fallback'
         WHERE state IN ('failed', 'ambiguous') AND fallback_decision IS NULL;",
    )?;
    terminalize_invalid_active_envelopes(connection)?;
    terminalize_missing_semantic_response_deliveries(connection)?;
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = 'delivery-schema-repair',
             updated_at_unix_ms = MAX(updated_at_unix_ms, COALESCE((
               SELECT delivery.updated_at_unix_ms
               FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
                 AND delivery.job_token = receiver_jobs.job_token
                 AND delivery.state IN ('failed', 'ambiguous')
               LIMIT 1
             ), updated_at_unix_ms))
         WHERE EXISTS (
           SELECT 1 FROM receiver_deliveries AS delivery
           WHERE delivery.job_id = receiver_jobs.job_id
             AND delivery.job_token = receiver_jobs.job_token
             AND delivery.state IN ('failed', 'ambiguous')
             AND NOT EXISTS (
               SELECT 1 FROM receiver_deliveries AS active
               WHERE active.job_id = receiver_jobs.job_id
                 AND active.job_token = receiver_jobs.job_token
                 AND ((receiver_jobs.state = 'answer-ready' AND active.state = 'ready')
                   OR (receiver_jobs.state = 'delivering' AND active.state = 'delivering')
                   OR (receiver_jobs.state = 'retrying' AND active.state = 'retrying'))
             )
         );",
    )?;
    Ok(())
}

fn normalize_frozen_fallbacks(connection: &Connection) -> Result<()> {
    let invalid = {
        let mut statement = connection
            .prepare("SELECT delivery_id, frozen_fallbacks_json FROM receiver_deliveries")?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(delivery_id, frozen)| {
                serde_json::from_str::<Vec<crate::state::ReceiverFallbackDestination>>(&frozen)
                    .is_err()
                    .then_some(delivery_id)
            })
            .collect::<Vec<_>>()
    };
    for delivery_id in invalid {
        connection.execute(
            "UPDATE receiver_deliveries SET frozen_fallbacks_json = '[]'
             WHERE delivery_id = ?1",
            [delivery_id],
        )?;
    }
    Ok(())
}

fn terminalize_missing_semantic_response_deliveries(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = 'delivery-schema-repair-missing'
         WHERE state IN ('answer-ready', 'delivering', 'retrying')
           AND (state != 'retrying' OR retry_from_state IS NULL)
           AND NOT EXISTS (
             SELECT 1 FROM receiver_deliveries AS delivery
             WHERE delivery.job_id = receiver_jobs.job_id
               AND delivery.job_token = receiver_jobs.job_token
               AND ((receiver_jobs.state = 'answer-ready' AND delivery.state = 'ready')
                 OR (receiver_jobs.state = 'delivering' AND delivery.state = 'delivering')
                 OR (receiver_jobs.state = 'retrying' AND delivery.state = 'retrying'))
           );",
    )?;
    Ok(())
}

fn terminalize_invalid_active_envelopes(connection: &Connection) -> Result<()> {
    let invalid = {
        let mut statement = connection.prepare(
            "SELECT delivery_id, envelope_json FROM receiver_deliveries
             WHERE state IN ('ready', 'delivering', 'retrying')",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(delivery_id, envelope)| {
                serde_json::from_str::<crate::state::ReceiverDeliveryEnvelope>(&envelope)
                    .is_err()
                    .then_some(delivery_id)
            })
            .collect::<Vec<_>>()
    };
    for delivery_id in invalid {
        connection.execute(
            "UPDATE receiver_deliveries
             SET state = 'failed', retry_at_unix_ms = NULL,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 provider_io_started = 0, provider_reference = NULL,
                 error_category = 'invalid-request', ambiguity_reason = NULL,
                 fallback_decision = 'no-safe-fallback'
             WHERE delivery_id = ?1
               AND state IN ('ready', 'delivering', 'retrying')",
            [delivery_id],
        )?;
    }
    Ok(())
}

fn ensure_table_contract(connection: &Connection) -> Result<()> {
    let sql: String = connection.query_row(
        "SELECT sql FROM sqlite_master
         WHERE type = 'table' AND name = 'receiver_deliveries'",
        [],
        |row| row.get(0),
    )?;
    if sql.contains(CURRENT_DELIVERY_CONTRACT) {
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
