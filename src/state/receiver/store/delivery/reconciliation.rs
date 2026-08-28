use anyhow::Result;

use super::super::to_i64;
use super::decode::{ExpiredDelivery, decode_expired_delivery, provider_for};
use super::result::apply_decision;
use crate::state::{
    Db, ReceiverDeliveryAmbiguity, ReceiverDeliveryPolicySnapshot, ReceiverProviderResultClass,
    decide_receiver_delivery,
};

impl Db {
    pub fn reconcile_expired_receiver_deliveries(&self, now_unix_ms: u64) -> Result<usize> {
        let now = to_i64(now_unix_ms, "receiver delivery reconciliation time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let expired = {
            let mut statement = transaction.prepare(
                "SELECT delivery.delivery_id, delivery.job_id, delivery.job_token,
                        delivery.attempt_id, delivery.claim_owner, delivery.envelope_json,
                        delivery.attempt_count, delivery.first_attempt_at_unix_ms,
                        delivery.provider_io_started
                 FROM receiver_deliveries AS delivery
                 JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
                  AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
                 WHERE delivery.response_kind = 'final-answer'
                   AND delivery.state = 'delivering' AND job.state = 'delivering'
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
        Ok(expired.len())
    }
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
