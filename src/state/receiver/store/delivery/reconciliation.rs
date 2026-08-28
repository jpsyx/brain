use anyhow::Result;

use super::super::to_i64;
use super::decode::{
    DueDelivery, ExpiredDelivery, decode_due_delivery, decode_expired_delivery, provider_for,
};
use super::result::apply_decision;
use crate::state::{
    Db, ReceiverDeliveryAmbiguity, ReceiverDeliveryPolicySnapshot, ReceiverProviderResultClass,
    decide_receiver_delivery, receiver_delivery_replay_window_is_expired,
};

impl Db {
    pub fn reconcile_expired_receiver_deliveries(&self, now_unix_ms: u64) -> Result<usize> {
        let now = to_i64(now_unix_ms, "receiver delivery reconciliation time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let migrated = migrate_legacy_unavailable_notices(&transaction, &self.workspace_id, now)?;
        let terminalized =
            terminalize_expired_due_retries(&transaction, &self.workspace_id, now_unix_ms)?;
        let expired = {
            let mut statement = transaction.prepare(
                "SELECT delivery.delivery_id, delivery.job_id, delivery.job_token,
                        delivery.attempt_id, delivery.claim_owner, delivery.envelope_json,
                        delivery.attempt_count, delivery.first_attempt_at_unix_ms,
                        delivery.provider_io_started
                 FROM receiver_deliveries AS delivery
                 JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
                  AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
                 WHERE delivery.state = 'delivering' AND job.state = 'delivering'
                   AND delivery.claim_expires_at_unix_ms <= ?2
                 ORDER BY delivery.created_at_unix_ms, delivery.delivery_id",
            )?;
            statement
                .query_map(
                    rusqlite::params![self.workspace_id, now],
                    decode_expired_delivery,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for delivery in &expired {
            if delivery.provider_io_started {
                let decision = decide_receiver_delivery(ReceiverDeliveryPolicySnapshot {
                    provider: provider_for(&delivery.envelope),
                    attempt_count: delivery.attempt_count,
                    first_attempt_at_unix_ms: delivery.first_attempt_at_unix_ms,
                    now_unix_ms,
                    result: ReceiverProviderResultClass::Ambiguous(
                        ReceiverDeliveryAmbiguity::ProviderAcceptanceUnknown,
                    ),
                });
                apply_decision(
                    &transaction,
                    &self.workspace_id,
                    delivery.delivery_id,
                    delivery.job_id,
                    delivery.token,
                    Some(delivery.attempt_id),
                    Some(&delivery.owner),
                    decision,
                    now_unix_ms,
                )?;
            } else {
                requeue_pre_spawn(&transaction, &self.workspace_id, delivery, now_unix_ms)?;
            }
        }
        transaction.commit()?;
        Ok(migrated
            .saturating_add(terminalized)
            .saturating_add(expired.len()))
    }
}

fn migrate_legacy_unavailable_notices(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    observed_at_unix_ms: i64,
) -> Result<usize> {
    let pending = {
        let mut statement = transaction.prepare(
            "SELECT job_id, job_token, inbound_json, response_sender
             FROM receiver_jobs
             WHERE workspace_id = ?1 AND state = 'failed'
               AND pending_unavailable_notice = 1
               AND recovery_cleanup_instance IS NULL
               AND recovery_cleanup_session_id IS NULL
             ORDER BY received_at_unix_ms, job_id",
        )?;
        statement
            .query_map([workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut migrated = 0usize;
    for (job_id, token, inbound_json, response_sender) in pending {
        let job_id = crate::state::ReceiverJobId::parse(&job_id)?;
        let token = crate::state::ReceiverJobToken::parse(&token)?;
        let inbound = super::super::decode_inbound(&inbound_json, response_sender)?;
        let message = crate::server::reply::unanswered_notice(
            super::super::response_intent::channel_label(inbound.channel),
        );
        if super::super::response_intent::insert(
            transaction,
            job_id,
            token,
            &inbound,
            crate::state::ReceiverResponseKind::UnavailableNotice,
            &message.text,
            observed_at_unix_ms,
        )
        .is_err()
        {
            transaction.execute(
                "UPDATE receiver_jobs
                 SET pending_unavailable_notice = 0,
                     last_error = 'notice-no-authorized-destination',
                     updated_at_unix_ms = ?4
                 WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
                   AND state = 'failed' AND pending_unavailable_notice = 1",
                rusqlite::params![
                    workspace_id,
                    job_id.to_string(),
                    token.to_string(),
                    observed_at_unix_ms
                ],
            )?;
            migrated = migrated.saturating_add(1);
            continue;
        }
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'answer-ready', pending_unavailable_notice = 0,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = 'failed' AND pending_unavailable_notice = 1
               AND EXISTS (SELECT 1 FROM receiver_deliveries
                 WHERE job_id = ?2 AND job_token = ?3
                   AND response_kind = 'unavailable-notice' AND state = 'ready')",
            rusqlite::params![
                workspace_id,
                job_id.to_string(),
                token.to_string(),
                observed_at_unix_ms
            ],
        )?;
        migrated = migrated.saturating_add(changed);
    }
    Ok(migrated)
}

fn terminalize_expired_due_retries(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    now_unix_ms: u64,
) -> Result<usize> {
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
    let mut terminalized = 0usize;
    for delivery in &due {
        terminalized = terminalized.saturating_add(usize::from(terminalize_expired_due_retry(
            transaction,
            workspace_id,
            delivery,
            now_unix_ms,
        )?));
    }
    Ok(terminalized)
}

pub(super) fn terminalize_expired_due_retry(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    delivery: &DueDelivery,
    now_unix_ms: u64,
) -> Result<bool> {
    if delivery.source_state != "retrying"
        || !receiver_delivery_replay_window_is_expired(
            provider_for(&delivery.envelope),
            delivery.attempt_count,
            delivery.first_attempt_at_unix_ms,
            now_unix_ms,
        )
    {
        return Ok(false);
    }
    let Some(retry_at_unix_ms) = delivery.retry_at_unix_ms else {
        return Ok(false);
    };
    let now = to_i64(
        now_unix_ms,
        "receiver delivery replay-window terminalization",
    )?;
    let delivery_changed = transaction.execute(
        "UPDATE receiver_deliveries
         SET state = 'ambiguous', retry_at_unix_ms = NULL,
             claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             provider_io_started = 0, provider_reference = NULL,
             error_category = NULL, ambiguity_reason = 'idempotency-window-expired',
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
        ],
    )?;
    if delivery_changed == 0 {
        return Ok(false);
    }
    let job_changed = transaction.execute(
        "UPDATE receiver_jobs
         SET state = 'failed', retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = NULL, updated_at_unix_ms = ?4
         WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND state = 'retrying'",
        rusqlite::params![
            workspace_id,
            delivery.job_id.to_string(),
            delivery.token.to_string(),
            now,
        ],
    )?;
    anyhow::ensure!(
        job_changed == 1,
        "receiver delivery replay-window reconciliation lost exact job authority"
    );
    Ok(true)
}

fn requeue_pre_spawn(
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
