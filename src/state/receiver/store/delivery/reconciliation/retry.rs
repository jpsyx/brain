use anyhow::Result;

use super::super::super::to_i64;
use super::super::decode::{DueDelivery, ExpiredDelivery, decode_due_delivery, provider_for};
use crate::state::receiver_delivery_replay_window_is_expired;

pub(super) fn terminalize_expired_due_retries(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    now_unix_ms: u64,
) -> Result<Vec<super::super::result::DeliveryLifecycle>> {
    let now = to_i64(
        now_unix_ms,
        "receiver delivery replay-window reconciliation",
    )?;
    let due = {
        let mut statement = transaction.prepare(
            "SELECT delivery.delivery_id, delivery.job_id, delivery.job_token,
                    delivery.envelope_json, delivery.attempt_count,
                    delivery.first_attempt_at_unix_ms,
                    delivery.retry_at_unix_ms, delivery.state
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
              AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
             WHERE delivery.state = 'retrying' AND job.state = 'retrying'
               AND delivery.retry_at_unix_ms <= ?2
             ORDER BY delivery.created_at_unix_ms, delivery.delivery_id",
        )?;
        statement
            .query_map(rusqlite::params![workspace_id, now], decode_due_delivery)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut terminalized = Vec::new();
    for delivery in &due {
        if let Some(lifecycle) =
            terminalize_expired_due_retry(transaction, workspace_id, delivery, now_unix_ms)?
        {
            terminalized.push(lifecycle);
        }
    }
    Ok(terminalized)
}

pub(in super::super) fn terminalize_expired_due_retry(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    delivery: &DueDelivery,
    now_unix_ms: u64,
) -> Result<Option<super::super::result::DeliveryLifecycle>> {
    if delivery.source_state != "retrying"
        || !receiver_delivery_replay_window_is_expired(
            provider_for(&delivery.envelope),
            delivery.attempt_count,
            delivery.first_attempt_at_unix_ms,
            now_unix_ms,
        )
    {
        return Ok(None);
    }
    let Some(retry_at_unix_ms) = delivery.retry_at_unix_ms else {
        return Ok(None);
    };
    let now = to_i64(
        now_unix_ms,
        "receiver delivery replay-window terminalization",
    )?;
    let fallback = super::super::result::terminal_fallback(transaction, delivery.delivery_id)?;
    let delivery_changed = transaction.execute(
        "UPDATE receiver_deliveries
         SET state = 'ambiguous', retry_at_unix_ms = NULL,
             claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             provider_io_started = 0, provider_reference = NULL,
             error_category = NULL, ambiguity_reason = 'idempotency-window-expired',
             fallback_decision = ?10,
             updated_at_unix_ms = ?8
         WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND state = 'retrying' AND attempt_count = ?4
           AND first_attempt_at_unix_ms = ?5 AND retry_at_unix_ms = ?6
           AND retry_at_unix_ms <= ?7 AND EXISTS (SELECT 1 FROM receiver_jobs
             WHERE workspace_id = ?9 AND job_id = ?2 AND job_token = ?3
               AND state = 'retrying')",
        rusqlite::params![
            delivery.delivery_id.to_string(),
            delivery.job_id.to_string(),
            delivery.token.to_string(),
            i64::from(delivery.attempt_count),
            delivery
                .first_attempt_at_unix_ms
                .and_then(|value| i64::try_from(value).ok()),
            to_i64(retry_at_unix_ms, "receiver delivery retry deadline")?,
            now,
            now,
            workspace_id,
            fallback.decision(),
        ],
    )?;
    if delivery_changed == 0 {
        return Ok(None);
    }
    fallback.insert_notice(transaction, delivery.job_id, delivery.token, now)?;
    let job_changed = transaction.execute(
        "UPDATE receiver_jobs
         SET state = ?4, retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = NULL, updated_at_unix_ms = ?5
         WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND state = 'retrying'",
        rusqlite::params![
            workspace_id,
            delivery.job_id.to_string(),
            delivery.token.to_string(),
            fallback.job_state(),
            now,
        ],
    )?;
    anyhow::ensure!(
        job_changed == 1,
        "receiver delivery replay-window reconciliation lost exact job authority"
    );
    Ok(Some(super::super::result::DeliveryLifecycle::new(
        "ambiguous",
        fallback.job_state(),
        crate::logging::ReceiverLifecycleReason::IdempotencyWindowExpired,
    )))
}

pub(super) fn requeue_pre_spawn(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    delivery: &ExpiredDelivery,
    now_unix_ms: u64,
) -> Result<()> {
    let now = to_i64(now_unix_ms, "receiver delivery requeue time")?;
    let (delivery_state, job_state, retry_at) = if delivery.attempt_count == 0 {
        ("ready", "answer-ready", None)
    } else {
        ("retrying", "retrying", Some(now))
    };
    let delivery_changed = transaction.execute(
        "UPDATE receiver_deliveries
         SET state = ?8, attempt_id = NULL, retry_at_unix_ms = ?9,
             claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             provider_io_started = 0, updated_at_unix_ms = ?10
         WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND attempt_id = ?4 AND claim_owner = ?5 AND state = 'delivering'
           AND provider_io_started = 0 AND EXISTS (SELECT 1 FROM receiver_jobs
             WHERE workspace_id = ?6 AND job_id = ?2 AND job_token = ?3
               AND state = 'delivering')",
        rusqlite::params![
            delivery.delivery_id.to_string(),
            delivery.job_id.to_string(),
            delivery.token.to_string(),
            delivery.attempt_id.to_string(),
            delivery.owner,
            workspace_id,
            now,
            delivery_state,
            retry_at,
            now,
        ],
    )?;
    let job_changed = transaction.execute(
        "UPDATE receiver_jobs SET state = ?4, updated_at_unix_ms = ?5
         WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND state = 'delivering'",
        rusqlite::params![
            workspace_id,
            delivery.job_id.to_string(),
            delivery.token.to_string(),
            job_state,
            now
        ],
    )?;
    anyhow::ensure!(
        delivery_changed == 1 && job_changed == 1,
        "receiver pre-spawn requeue compare-and-swap lost authority"
    );
    Ok(())
}
