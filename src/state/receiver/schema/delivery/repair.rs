use anyhow::Result;
use rusqlite::Connection;

pub(super) fn reconcile_rows(connection: &Connection) -> Result<()> {
    super::structural_repair::repair_structurally_malformed_deliveries(connection)?;
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
    super::fallback_success::restore_acknowledged_jobs(connection)?;
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
