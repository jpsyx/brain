use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::super::{to_i64, validated_owner};
use super::decode::decode_due_delivery;
use crate::state::{Db, ReceiverDeliveryAttemptId, ReceiverDeliveryClaim};

impl Db {
    /// Claim the oldest due final-answer delivery without touching the agent claim lane.
    pub fn claim_next_receiver_delivery(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverDeliveryClaim>> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver delivery claim expiry must be in the future"
        );
        let now = to_i64(now_unix_ms, "receiver delivery claim time")?;
        let expires = to_i64(expires_at_unix_ms, "receiver delivery claim expiry")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let due = transaction
            .query_row(
                "SELECT delivery.delivery_id, delivery.job_id, delivery.job_token,
                        delivery.envelope_json, delivery.attempt_count,
                        delivery.first_attempt_at_unix_ms, delivery.state
                 FROM receiver_deliveries AS delivery
                 JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
                  AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
                 WHERE delivery.response_kind = 'final-answer'
                   AND ((delivery.state = 'ready' AND job.state = 'answer-ready')
                     OR (delivery.state = 'retrying' AND job.state = 'retrying'
                       AND delivery.retry_at_unix_ms <= ?2))
                 ORDER BY delivery.created_at_unix_ms, delivery.delivery_id
                 LIMIT 1",
                rusqlite::params![self.workspace_id, now],
                decode_due_delivery,
            )
            .optional()?;
        let Some(due) = due else {
            return Ok(None);
        };
        let attempt_id = ReceiverDeliveryAttemptId::new();
        let delivery_changed = transaction.execute(
            "UPDATE receiver_deliveries
             SET state = 'delivering', attempt_id = ?2, claim_owner = ?3,
                 claim_expires_at_unix_ms = ?4, provider_io_started = 0,
                 retry_at_unix_ms = NULL, provider_reference = NULL,
                 error_category = NULL, ambiguity_reason = NULL, updated_at_unix_ms = ?5
             WHERE delivery_id = ?1 AND job_id = ?6 AND job_token = ?7
               AND state = ?8",
            rusqlite::params![
                due.delivery_id.to_string(),
                attempt_id.to_string(),
                owner,
                expires,
                now,
                due.job_id.to_string(),
                due.token.to_string(),
                due.source_state,
            ],
        )?;
        let expected_job_state = if due.source_state == "ready" {
            "answer-ready"
        } else {
            "retrying"
        };
        let job_changed = transaction.execute(
            "UPDATE receiver_jobs SET state = 'delivering', updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND state = ?5",
            rusqlite::params![
                self.workspace_id,
                due.job_id.to_string(),
                due.token.to_string(),
                now,
                expected_job_state,
            ],
        )?;
        if delivery_changed != 1 || job_changed != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(ReceiverDeliveryClaim::new(
            due.delivery_id,
            attempt_id,
            due.job_id,
            due.token,
            owner.to_owned(),
            expires_at_unix_ms,
            due.attempt_count.saturating_add(1),
            due.first_attempt_at_unix_ms,
            due.envelope,
        )))
    }

    /// Persist the no-replay boundary immediately before provider work is published.
    pub fn mark_receiver_delivery_io_started(
        &self,
        claim: &ReceiverDeliveryClaim,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let observed = to_i64(observed_at_unix_ms, "receiver delivery IO start")?;
        Ok(self.conn.execute(
            "UPDATE receiver_deliveries
             SET provider_io_started = 1, attempt_count = attempt_count + 1,
                 first_attempt_at_unix_ms = COALESCE(first_attempt_at_unix_ms, ?8),
                 updated_at_unix_ms = ?8
             WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND attempt_id = ?4 AND claim_owner = ?5
               AND claim_expires_at_unix_ms = ?6 AND claim_expires_at_unix_ms > ?7
               AND state = 'delivering' AND provider_io_started = 0
               AND EXISTS (SELECT 1 FROM receiver_jobs
                 WHERE workspace_id = ?9 AND job_id = ?2 AND job_token = ?3
                   AND state = 'delivering')",
            rusqlite::params![
                claim.delivery_id().to_string(),
                claim.job_id().to_string(),
                claim.token().to_string(),
                claim.attempt_id().to_string(),
                claim.owner(),
                to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                observed,
                observed,
                self.workspace_id,
            ],
        )? == 1)
    }

    /// Release an exact reservation only while durable state proves provider IO never began.
    pub fn release_receiver_delivery_before_io(
        &self,
        claim: &ReceiverDeliveryClaim,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let observed = to_i64(observed_at_unix_ms, "receiver delivery reservation release")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let attempt_count = transaction
            .query_row(
                "SELECT delivery.attempt_count
                 FROM receiver_deliveries AS delivery
                 JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
                  AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
                 WHERE delivery.delivery_id = ?2 AND delivery.job_id = ?3
                   AND delivery.job_token = ?4 AND delivery.attempt_id = ?5
                   AND delivery.claim_owner = ?6 AND delivery.claim_expires_at_unix_ms = ?7
                   AND delivery.state = 'delivering' AND delivery.provider_io_started = 0
                   AND job.state = 'delivering'",
                rusqlite::params![
                    self.workspace_id,
                    claim.delivery_id().to_string(),
                    claim.job_id().to_string(),
                    claim.token().to_string(),
                    claim.attempt_id().to_string(),
                    claim.owner(),
                    to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(attempt_count) = attempt_count else {
            return Ok(false);
        };
        let delivery_state = if attempt_count == 0 {
            "ready"
        } else {
            "retrying"
        };
        let job_state = if attempt_count == 0 {
            "answer-ready"
        } else {
            "retrying"
        };
        let delivery_changed = transaction.execute(
            "UPDATE receiver_deliveries
             SET state = ?8, attempt_id = NULL, retry_at_unix_ms = ?9,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 provider_io_started = 0, updated_at_unix_ms = ?10
             WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND attempt_id = ?4 AND claim_owner = ?5 AND claim_expires_at_unix_ms = ?6
               AND state = 'delivering' AND provider_io_started = 0",
            rusqlite::params![
                claim.delivery_id().to_string(),
                claim.job_id().to_string(),
                claim.token().to_string(),
                claim.attempt_id().to_string(),
                claim.owner(),
                to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                self.workspace_id,
                delivery_state,
                (attempt_count > 0).then_some(observed),
                observed,
            ],
        )?;
        let job_changed = transaction.execute(
            "UPDATE receiver_jobs SET state = ?4, updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = 'delivering'",
            rusqlite::params![
                self.workspace_id,
                claim.job_id().to_string(),
                claim.token().to_string(),
                job_state,
                observed,
            ],
        )?;
        if delivery_changed != 1 || job_changed != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Undo the IO marker when publication proves the worker vanished before execution.
    pub fn release_receiver_delivery_after_failed_publication(
        &self,
        claim: &ReceiverDeliveryClaim,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let observed = to_i64(
            observed_at_unix_ms,
            "receiver delivery failed publication release",
        )?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let durable_attempts = transaction
            .query_row(
                "SELECT delivery.attempt_count
                 FROM receiver_deliveries AS delivery
                 JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
                  AND job.workspace_id = ?1 AND job.job_token = delivery.job_token
                 WHERE delivery.delivery_id = ?2 AND delivery.job_id = ?3
                   AND delivery.job_token = ?4 AND delivery.attempt_id = ?5
                   AND delivery.claim_owner = ?6 AND delivery.claim_expires_at_unix_ms = ?7
                   AND delivery.state = 'delivering' AND delivery.provider_io_started = 1
                   AND delivery.attempt_count = ?8 AND job.state = 'delivering'",
                rusqlite::params![
                    self.workspace_id,
                    claim.delivery_id().to_string(),
                    claim.job_id().to_string(),
                    claim.token().to_string(),
                    claim.attempt_id().to_string(),
                    claim.owner(),
                    to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                    i64::from(claim.attempt_count()),
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(durable_attempts) = durable_attempts else {
            return Ok(false);
        };
        anyhow::ensure!(
            durable_attempts > 0,
            "receiver delivery publication attempt count is invalid"
        );
        let previous_attempts = durable_attempts - 1;
        let (delivery_state, job_state, retry_at) = if previous_attempts == 0 {
            ("ready", "answer-ready", None)
        } else {
            ("retrying", "retrying", Some(observed))
        };
        let delivery_changed = transaction.execute(
            "UPDATE receiver_deliveries
             SET state = ?9, attempt_id = NULL, attempt_count = ?10,
                 retry_at_unix_ms = ?11, claim_owner = NULL,
                 claim_expires_at_unix_ms = NULL, provider_io_started = 0,
                 first_attempt_at_unix_ms = CASE WHEN ?10 = 0 THEN NULL
                                                 ELSE first_attempt_at_unix_ms END,
                 updated_at_unix_ms = ?12
             WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND attempt_id = ?4 AND claim_owner = ?5 AND claim_expires_at_unix_ms = ?6
               AND state = 'delivering' AND provider_io_started = 1
               AND attempt_count = ?7 AND EXISTS (SELECT 1 FROM receiver_jobs
                 WHERE workspace_id = ?8 AND job_id = ?2 AND job_token = ?3
                   AND state = 'delivering')",
            rusqlite::params![
                claim.delivery_id().to_string(),
                claim.job_id().to_string(),
                claim.token().to_string(),
                claim.attempt_id().to_string(),
                claim.owner(),
                to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                durable_attempts,
                self.workspace_id,
                delivery_state,
                previous_attempts,
                retry_at,
                observed,
            ],
        )?;
        let job_changed = transaction.execute(
            "UPDATE receiver_jobs SET state = ?4, updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = 'delivering'",
            rusqlite::params![
                self.workspace_id,
                claim.job_id().to_string(),
                claim.token().to_string(),
                job_state,
                observed,
            ],
        )?;
        if delivery_changed != 1 || job_changed != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}
