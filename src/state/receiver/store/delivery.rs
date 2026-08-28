use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{
    Db, ReceiverDeliveryAmbiguity, ReceiverDeliveryApplyOutcome, ReceiverDeliveryAttemptId,
    ReceiverDeliveryClaim, ReceiverDeliveryDecision, ReceiverDeliveryEnvelope, ReceiverDeliveryId,
    ReceiverDeliveryPolicySnapshot, ReceiverJobId, ReceiverJobToken, ReceiverProviderCapability,
    ReceiverProviderResultClass, decide_receiver_delivery,
};

struct DueDelivery {
    delivery_id: ReceiverDeliveryId,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    envelope: ReceiverDeliveryEnvelope,
    attempt_count: u32,
    first_attempt_at_unix_ms: Option<u64>,
    source_state: String,
}

struct ExpiredDelivery {
    delivery_id: ReceiverDeliveryId,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    attempt_id: ReceiverDeliveryAttemptId,
    owner: String,
    envelope: ReceiverDeliveryEnvelope,
    attempt_count: u32,
    first_attempt_at_unix_ms: Option<u64>,
    provider_io_started: bool,
}

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

    /// Apply one typed provider result through the exact live delivery claim.
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

    /// Reconcile every expired delivery claim from durable pre-spawn/IO facts.
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

fn decode_due_delivery(row: &rusqlite::Row<'_>) -> rusqlite::Result<DueDelivery> {
    decode_delivery_parts(row).map(
        |(delivery_id, job_id, token, envelope, attempt_count, first_attempt_at_unix_ms)| {
            DueDelivery {
                delivery_id,
                job_id,
                token,
                envelope,
                attempt_count,
                first_attempt_at_unix_ms,
                source_state: row.get(6).unwrap_or_default(),
            }
        },
    )
}

type DeliveryParts = (
    ReceiverDeliveryId,
    ReceiverJobId,
    ReceiverJobToken,
    ReceiverDeliveryEnvelope,
    u32,
    Option<u64>,
);

fn decode_delivery_parts(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeliveryParts> {
    let delivery_id = parse_sql(0, &row.get::<_, String>(0)?, ReceiverDeliveryId::parse)?;
    let job_id = parse_sql(1, &row.get::<_, String>(1)?, ReceiverJobId::parse)?;
    let token = parse_sql(2, &row.get::<_, String>(2)?, ReceiverJobToken::parse)?;
    let envelope_json = row.get::<_, String>(3)?;
    let envelope =
        serde_json::from_str(&envelope_json).map_err(|error| sql_decode_error(3, error))?;
    let attempt_count =
        u32::try_from(row.get::<_, i64>(4)?).map_err(|error| sql_decode_error(4, error))?;
    let first_attempt_at_unix_ms = row
        .get::<_, Option<i64>>(5)?
        .map(u64::try_from)
        .transpose()
        .map_err(|error| sql_decode_error(5, error))?;
    Ok((
        delivery_id,
        job_id,
        token,
        envelope,
        attempt_count,
        first_attempt_at_unix_ms,
    ))
}

fn decode_expired_delivery(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExpiredDelivery> {
    let delivery_id = parse_sql(0, &row.get::<_, String>(0)?, ReceiverDeliveryId::parse)?;
    let job_id = parse_sql(1, &row.get::<_, String>(1)?, ReceiverJobId::parse)?;
    let token = parse_sql(2, &row.get::<_, String>(2)?, ReceiverJobToken::parse)?;
    let attempt_id = parse_sql(
        3,
        &row.get::<_, String>(3)?,
        ReceiverDeliveryAttemptId::parse,
    )?;
    let envelope_json = row.get::<_, String>(5)?;
    let envelope =
        serde_json::from_str(&envelope_json).map_err(|error| sql_decode_error(5, error))?;
    Ok(ExpiredDelivery {
        delivery_id,
        job_id,
        token,
        attempt_id,
        owner: row.get(4)?,
        envelope,
        attempt_count: u32::try_from(row.get::<_, i64>(6)?)
            .map_err(|error| sql_decode_error(6, error))?,
        first_attempt_at_unix_ms: row
            .get::<_, Option<i64>>(7)?
            .map(u64::try_from)
            .transpose()
            .map_err(|error| sql_decode_error(7, error))?,
        provider_io_started: row.get(8)?,
    })
}

fn parse_sql<T>(
    index: usize,
    value: &str,
    parse: impl FnOnce(&str) -> anyhow::Result<T>,
) -> rusqlite::Result<T> {
    parse(value).map_err(|_| {
        sql_decode_error(
            index,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "receiver delivery identity is invalid",
            ),
        )
    })
}

fn sql_decode_error(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(error))
}

fn provider_for(envelope: &ReceiverDeliveryEnvelope) -> ReceiverProviderCapability {
    match envelope {
        ReceiverDeliveryEnvelope::Sms { .. } => ReceiverProviderCapability::Twilio,
        ReceiverDeliveryEnvelope::Email { .. } => ReceiverProviderCapability::Resend,
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
fn apply_decision(
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
