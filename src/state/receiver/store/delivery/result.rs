use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use super::super::to_i64;
use super::decode::{provider_for, sql_decode_error};
use crate::state::{
    Db, ReceiverDeliveryApplyOutcome, ReceiverDeliveryAttemptId, ReceiverDeliveryClaim,
    ReceiverDeliveryDecision, ReceiverDeliveryEnvelope, ReceiverDeliveryId,
    ReceiverDeliveryPolicySnapshot, ReceiverJobId, ReceiverJobToken, ReceiverProviderResultClass,
    decide_receiver_delivery,
};

impl Db {
    pub fn apply_receiver_delivery_result(
        &self,
        claim: &ReceiverDeliveryClaim,
        observed_at_unix_ms: u64,
        result: ReceiverProviderResultClass,
    ) -> Result<ReceiverDeliveryApplyOutcome> {
        let observed = to_i64(observed_at_unix_ms, "receiver delivery result time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let durable = exact_live_attempt(&transaction, &self.workspace_id, claim, observed)?;
        let Some((attempt_count, first_attempt_at_unix_ms, envelope)) = durable else {
            return Ok(ReceiverDeliveryApplyOutcome::Stale);
        };
        let decision = decide_receiver_delivery(ReceiverDeliveryPolicySnapshot {
            provider: provider_for(&envelope),
            attempt_count,
            first_attempt_at_unix_ms,
            now_unix_ms: observed_at_unix_ms,
            result,
        });
        apply_decision(
            &transaction,
            &self.workspace_id,
            claim.delivery_id(),
            claim.job_id(),
            claim.token(),
            Some(claim.attempt_id()),
            Some(claim.owner()),
            decision,
            observed_at_unix_ms,
        )?;
        transaction.commit()?;
        Ok(ReceiverDeliveryApplyOutcome::Applied)
    }
}

fn exact_live_attempt(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    claim: &ReceiverDeliveryClaim,
    observed: i64,
) -> Result<Option<(u32, Option<u64>, ReceiverDeliveryEnvelope)>> {
    transaction
        .query_row(
            "SELECT delivery.attempt_count, delivery.first_attempt_at_unix_ms,
                    delivery.envelope_json
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
              AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
             WHERE delivery.delivery_id = ?2 AND delivery.job_id = ?3
               AND delivery.job_token = ?4 AND delivery.attempt_id = ?5
               AND delivery.claim_owner = ?6 AND delivery.claim_expires_at_unix_ms = ?7
               AND delivery.claim_expires_at_unix_ms > ?8
               AND delivery.state = 'delivering' AND delivery.provider_io_started = 1
               AND job.state = 'delivering'",
            rusqlite::params![
                workspace_id,
                claim.delivery_id().to_string(),
                claim.job_id().to_string(),
                claim.token().to_string(),
                claim.attempt_id().to_string(),
                claim.owner(),
                to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                observed,
            ],
            |row| {
                let attempt_count = u32::try_from(row.get::<_, i64>(0)?)
                    .map_err(|error| sql_decode_error(0, error))?;
                let first_attempt = row
                    .get::<_, Option<i64>>(1)?
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|error| sql_decode_error(1, error))?;
                let envelope = serde_json::from_str(&row.get::<_, String>(2)?)
                    .map_err(|error| sql_decode_error(2, error))?;
                Ok((attempt_count, first_attempt, envelope))
            },
        )
        .optional()
        .context("load exact receiver delivery attempt")
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_decision(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    delivery_id: ReceiverDeliveryId,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    attempt_id: Option<ReceiverDeliveryAttemptId>,
    owner: Option<&str>,
    decision: ReceiverDeliveryDecision,
    observed_at_unix_ms: u64,
) -> Result<()> {
    let observed = to_i64(observed_at_unix_ms, "receiver delivery result time")?;
    let (delivery_state, job_state, retry_at, provider_reference, error_category, ambiguity) =
        match decision {
            ReceiverDeliveryDecision::Acknowledged(reference) => (
                "acknowledged",
                "done",
                None,
                Some(reference.as_str().to_owned()),
                None,
                None,
            ),
            ReceiverDeliveryDecision::RetryAt {
                retry_at_unix_ms,
                error_category,
            } => (
                "retrying",
                "retrying",
                Some(to_i64(
                    retry_at_unix_ms,
                    "receiver delivery retry deadline",
                )?),
                None,
                Some(error_category.as_str()),
                None,
            ),
            ReceiverDeliveryDecision::TerminalFailure(category) => (
                "failed",
                "failed",
                None,
                None,
                Some(category.as_str()),
                None,
            ),
            ReceiverDeliveryDecision::TerminalAmbiguous(reason) => (
                "ambiguous",
                "failed",
                None,
                None,
                None,
                Some(reason.as_str()),
            ),
        };
    let delivery_changed = transaction.execute(
        "UPDATE receiver_deliveries
         SET state = ?8, retry_at_unix_ms = ?9, claim_owner = NULL,
             claim_expires_at_unix_ms = NULL, provider_io_started = 0,
             provider_reference = ?10, error_category = ?11,
             ambiguity_reason = ?12, updated_at_unix_ms = ?13
         WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND (?4 IS NULL OR attempt_id = ?4) AND (?5 IS NULL OR claim_owner = ?5)
           AND state = 'delivering' AND EXISTS (SELECT 1 FROM receiver_jobs
             WHERE workspace_id = ?6 AND job_id = ?2 AND job_token = ?3
               AND state = 'delivering')",
        rusqlite::params![
            delivery_id.to_string(),
            job_id.to_string(),
            token.to_string(),
            attempt_id.map(|value| value.to_string()),
            owner,
            workspace_id,
            observed,
            delivery_state,
            retry_at,
            provider_reference,
            error_category,
            ambiguity,
            observed,
        ],
    )?;
    let job_changed = transaction.execute(
        "UPDATE receiver_jobs SET state = ?4, retry_at_unix_ms = NULL,
             retry_from_state = NULL, last_error = NULL, updated_at_unix_ms = ?5
         WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND state = 'delivering'",
        rusqlite::params![
            workspace_id,
            job_id.to_string(),
            token.to_string(),
            job_state,
            observed
        ],
    )?;
    anyhow::ensure!(
        delivery_changed == 1 && job_changed == 1,
        "receiver delivery result compare-and-swap lost authority"
    );
    Ok(())
}
