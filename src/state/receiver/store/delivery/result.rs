use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use super::super::to_i64;
use super::decode::{provider_for, sql_decode_error};
use crate::state::{
    Db, ReceiverDeliveryApplyOutcome, ReceiverDeliveryAttemptId, ReceiverDeliveryClaim,
    ReceiverDeliveryDecision, ReceiverDeliveryEnvelope, ReceiverDeliveryId,
    ReceiverDeliveryPolicySnapshot, ReceiverFallbackPlan, ReceiverJobId, ReceiverJobToken,
    ReceiverProviderResultClass, ReceiverResponseKind, decide_receiver_delivery,
    plan_receiver_fallback,
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

    pub fn apply_receiver_delivery_result_before_io(
        &self,
        claim: &ReceiverDeliveryClaim,
        observed_at_unix_ms: u64,
        result: ReceiverProviderResultClass,
    ) -> Result<ReceiverDeliveryApplyOutcome> {
        anyhow::ensure!(
            matches!(
                result,
                ReceiverProviderResultClass::DefinitelyNotAccepted(_)
            ),
            "a no-IO receiver delivery result must be definitely not accepted"
        );
        let observed = to_i64(observed_at_unix_ms, "receiver delivery no-IO result time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let durable =
            exact_live_attempt_before_io(&transaction, &self.workspace_id, claim, observed)?;
        let Some((attempt_count, first_attempt_at_unix_ms, envelope)) = durable else {
            return Ok(ReceiverDeliveryApplyOutcome::Stale);
        };
        let attempt_count = attempt_count.saturating_add(1);
        let first_attempt_at_unix_ms = first_attempt_at_unix_ms.or(Some(observed_at_unix_ms));
        let changed = transaction.execute(
            "UPDATE receiver_deliveries
             SET attempt_count = ?8, first_attempt_at_unix_ms = ?9,
                 updated_at_unix_ms = ?10
             WHERE delivery_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND attempt_id = ?4 AND claim_owner = ?5
               AND claim_expires_at_unix_ms = ?6 AND claim_expires_at_unix_ms > ?7
               AND state = 'delivering' AND provider_io_started = 0",
            rusqlite::params![
                claim.delivery_id().to_string(),
                claim.job_id().to_string(),
                claim.token().to_string(),
                claim.attempt_id().to_string(),
                claim.owner(),
                to_i64(claim.expires_at_unix_ms(), "receiver delivery claim expiry")?,
                observed,
                i64::from(attempt_count),
                first_attempt_at_unix_ms
                    .map(|value| to_i64(value, "receiver delivery first attempt"))
                    .transpose()?,
                observed,
            ],
        )?;
        if changed != 1 {
            return Ok(ReceiverDeliveryApplyOutcome::Stale);
        }
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

fn exact_live_attempt_before_io(
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
               AND delivery.state = 'delivering' AND delivery.provider_io_started = 0
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
            decode_live_attempt,
        )
        .optional()
        .context("load exact receiver delivery attempt before provider IO")
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
            decode_live_attempt,
        )
        .optional()
        .context("load exact receiver delivery attempt")
}

fn decode_live_attempt(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(u32, Option<u64>, ReceiverDeliveryEnvelope)> {
    let attempt_count =
        u32::try_from(row.get::<_, i64>(0)?).map_err(|error| sql_decode_error(0, error))?;
    let first_attempt = row
        .get::<_, Option<i64>>(1)?
        .map(u64::try_from)
        .transpose()
        .map_err(|error| sql_decode_error(1, error))?;
    let envelope = serde_json::from_str(&row.get::<_, String>(2)?)
        .map_err(|error| sql_decode_error(2, error))?;
    Ok((attempt_count, first_attempt, envelope))
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
    let fallback = matches!(
        decision,
        ReceiverDeliveryDecision::TerminalFailure(_)
            | ReceiverDeliveryDecision::TerminalAmbiguous(_)
    )
    .then(|| terminal_fallback(transaction, delivery_id))
    .transpose()?;
    let fallback_decision = fallback.as_ref().map(TerminalFallback::decision);
    let (delivery_state, mut job_state, retry_at, provider_reference, error_category, ambiguity) =
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
    if let Some(fallback) = &fallback {
        job_state = fallback.job_state();
    }
    let delivery_changed = transaction.execute(
        "UPDATE receiver_deliveries
         SET state = ?8, retry_at_unix_ms = ?9, claim_owner = NULL,
             claim_expires_at_unix_ms = NULL, provider_io_started = 0,
             provider_reference = ?10, error_category = ?11,
             ambiguity_reason = ?12, fallback_decision = ?13,
             updated_at_unix_ms = ?14
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
            fallback_decision,
            observed,
        ],
    )?;
    if let Some(fallback) = &fallback {
        fallback.insert_notice(transaction, job_id, token, observed)?;
    }
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

pub(super) struct TerminalFallback {
    decision: &'static str,
    plan: Option<ReceiverFallbackPlan>,
}

impl TerminalFallback {
    pub(super) const fn decision(&self) -> &'static str {
        self.decision
    }

    pub(super) const fn job_state(&self) -> &'static str {
        if self.plan.is_some() {
            "answer-ready"
        } else {
            "failed"
        }
    }

    pub(super) fn insert_notice(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        observed_at_unix_ms: i64,
    ) -> Result<()> {
        let Some(plan) = &self.plan else {
            return Ok(());
        };
        let envelope = crate::state::receiver::fallback::render_receiver_fallback(plan);
        let envelope_json = serde_json::to_string(&envelope)
            .context("serialize frozen receiver fallback notice")?;
        let inserted = transaction.execute(
            "INSERT INTO receiver_deliveries
               (delivery_id, job_id, job_token, response_kind, envelope_json,
                state, attempt_count, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, 'fallback-notice', ?4, 'ready', 0, ?5, ?5)",
            rusqlite::params![
                ReceiverDeliveryId::new().to_string(),
                job_id.to_string(),
                token.to_string(),
                envelope_json,
                observed_at_unix_ms,
            ],
        )?;
        anyhow::ensure!(
            inserted == 1,
            "receiver fallback notice insert lost authority"
        );
        Ok(())
    }
}

pub(super) fn terminal_fallback(
    transaction: &rusqlite::Transaction<'_>,
    delivery_id: ReceiverDeliveryId,
) -> Result<TerminalFallback> {
    let (response_kind, envelope_json, frozen_json): (String, String, String) = transaction
        .query_row(
            "SELECT response_kind, envelope_json, frozen_fallbacks_json
             FROM receiver_deliveries WHERE delivery_id = ?1",
            [delivery_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    let response_kind = ReceiverResponseKind::parse(&response_kind)
        .context("decode terminal receiver response kind")?;
    if response_kind == ReceiverResponseKind::FallbackNotice {
        return Ok(TerminalFallback {
            decision: "no-safe-fallback",
            plan: None,
        });
    }
    let envelope: ReceiverDeliveryEnvelope =
        serde_json::from_str(&envelope_json).context("decode terminal receiver envelope")?;
    let frozen =
        serde_json::from_str::<Vec<crate::state::ReceiverFallbackDestination>>(&frozen_json)
            .context("decode frozen receiver fallback authority")?;
    let attempted = match &envelope {
        ReceiverDeliveryEnvelope::Sms { value } => vec![value.recipient()],
        ReceiverDeliveryEnvelope::Email { value } => {
            value.recipients().iter().map(String::as_str).collect()
        }
    };
    let plan = plan_receiver_fallback(provider_for(&envelope), &attempted, &frozen);
    Ok(TerminalFallback {
        decision: if plan.is_some() {
            "fallback-planned"
        } else {
            "no-safe-fallback"
        },
        plan,
    })
}
