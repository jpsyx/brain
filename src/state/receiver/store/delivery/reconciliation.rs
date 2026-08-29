use anyhow::Result;

use super::super::to_i64;
use super::decode::{decode_expired_delivery, provider_for};
use super::result::apply_decision;
use crate::state::{
    Db, ReceiverDeliveryAmbiguity, ReceiverDeliveryPolicySnapshot, ReceiverProviderResultClass,
    decide_receiver_delivery,
};

mod repair;
mod retry;

use repair::terminalize_invalid_semantic_responses;
pub(super) use retry::terminalize_expired_due_retry;
use retry::{requeue_pre_spawn, terminalize_expired_due_retries};

impl Db {
    pub fn reconcile_expired_receiver_deliveries(&self, now_unix_ms: u64) -> Result<usize> {
        let now = to_i64(now_unix_ms, "receiver delivery reconciliation time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let structurally_invalid =
            crate::state::receiver::schema::repair_structurally_malformed_deliveries(&transaction)?;
        let invalid_lifecycle =
            terminalize_invalid_semantic_responses(&transaction, &self.workspace_id, now)?;
        let terminalized_lifecycle =
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
        let invalid = invalid_lifecycle.len();
        let terminalized = terminalized_lifecycle.len();
        let mut lifecycle = invalid_lifecycle;
        lifecycle.extend(terminalized_lifecycle);
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
                lifecycle.push(apply_decision(
                    &transaction,
                    &self.workspace_id,
                    delivery.delivery_id,
                    delivery.job_id,
                    delivery.token,
                    Some(delivery.attempt_id),
                    Some(&delivery.owner),
                    decision,
                    now_unix_ms,
                )?);
            } else {
                requeue_pre_spawn(&transaction, &self.workspace_id, delivery, now_unix_ms)?;
            }
        }
        transaction.commit()?;
        for event in lifecycle {
            event.log(self);
        }
        Ok(structurally_invalid
            .saturating_add(invalid)
            .saturating_add(terminalized)
            .saturating_add(expired.len()))
    }
}
