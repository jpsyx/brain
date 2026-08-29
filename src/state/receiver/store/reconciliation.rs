//! Atomic durable receiver recovery reconciliation.

use anyhow::Result;

use super::{load::load_receiver_job, to_i64, validated_owner};
use crate::state::{
    Db, ReceiverJobId, ReceiverJobState, ReceiverReconciliationAction,
    ReceiverReconciliationEffect, ReceiverReconciliationReason, ReceiverRecoveryDecision,
    ReceiverRecoveryFailure, decide_receiver_recovery, receiver_recovery_expires_at,
};

mod cleanup;
mod recovery_registration;
mod spawned_cleanup;
mod support;
mod terminal;

use support::{
    EXACT_SNAPSHOT_SQL, RecoverySessionAttribution, attribute_exact_recovery_session,
    candidate_for_job, oldest_blocking_candidate, release_registration,
};
use terminal::{
    terminal_reason, terminalize, terminalize_launched_recovery,
    terminalize_without_observed_cleanup,
};

const PRE_ACCEPTANCE_RETRY_DELAY_MS: u64 = 5_000;

impl Db {
    /// Reconcile at most one oldest blocking receiver job in one immediate transaction.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed durable state, an unrepresentable timestamp,
    /// or a database failure.
    pub fn reconcile_next_receiver_job(
        &self,
        now_unix_ms: u64,
    ) -> Result<Option<ReceiverReconciliationEffect>> {
        let now = to_i64(now_unix_ms, "receiver reconciliation time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let Some(candidate) = oldest_blocking_candidate(&transaction, &self.workspace_id)? else {
            return cleanup::pending_cleanup_effect(&transaction, &self.workspace_id);
        };
        let Some(job) = load_receiver_job(&transaction, &self.workspace_id, candidate.job_id)?
        else {
            return Ok(None);
        };
        match decide_receiver_recovery(job.recovery_snapshot(now_unix_ms)) {
            ReceiverRecoveryDecision::Wait => {
                cleanup::pending_cleanup_effect(&transaction, &self.workspace_id)
            }
            ReceiverRecoveryDecision::RequeuePreAcceptance => {
                let retry_at = to_i64(
                    now_unix_ms.saturating_add(PRE_ACCEPTANCE_RETRY_DELAY_MS),
                    "receiver reconciled retry time",
                )?;
                let retry_from = match candidate.state {
                    ReceiverJobState::Claimed => "claimed",
                    ReceiverJobState::Launching | ReceiverJobState::Launched => "launching",
                    _ => return Ok(None),
                };
                let cleanup_instance = job
                    .observation_instance()
                    .map(str::to_owned)
                    .or_else(|| candidate.owner.clone());
                let cleanup_session_id = job.observation_session_id().map(str::to_owned);
                let sql = format!(
                    "UPDATE receiver_jobs
                     SET state = 'retrying', retry_count = retry_count + 1,
                         retry_at_unix_ms = ?6, retry_from_state = ?7,
                         last_error = ?8, claim_owner = NULL,
                         claim_expires_at_unix_ms = NULL,
                         launched_at_unix_ms = NULL,
                         observation_instance = NULL, observation_session_id = NULL,
                         observation_revision = 0,
                         attempt_accepted_at_unix_ms = NULL,
                         attempt_progressing_at_unix_ms = NULL,
                         latest_progress_at_unix_ms = NULL,
                         launch_expires_at_unix_ms = NULL,
                         acceptance_expires_at_unix_ms = NULL,
                         progress_expires_at_unix_ms = NULL,
                         recovery_expires_at_unix_ms = NULL,
                         updated_at_unix_ms = ?5
                     WHERE workspace_id = ?1 AND job_id = ?2 AND state = ?3
                       AND claim_owner IS ?4 AND claim_expires_at_unix_ms IS ?9
                       AND updated_at_unix_ms = ?10
                       AND {EXACT_SNAPSHOT_SQL} = ?11"
                );
                let changed = transaction.execute(
                    &sql,
                    rusqlite::params![
                        self.workspace_id,
                        candidate.job_id.to_string(),
                        candidate.state.as_str(),
                        candidate.owner,
                        now,
                        retry_at,
                        retry_from,
                        ReceiverReconciliationReason::PreAcceptanceTimeout.as_str(),
                        candidate.claim_expires_at_unix_ms,
                        candidate.updated_at_unix_ms,
                        candidate.exact_snapshot,
                    ],
                )?;
                if changed != 1 {
                    return Ok(None);
                }
                release_registration(
                    &transaction,
                    &self.workspace_id,
                    job.conversation_id(),
                    cleanup_instance.as_deref(),
                    now,
                )?;
                transaction.commit()?;
                Ok(Some(ReceiverReconciliationEffect::new(
                    ReceiverReconciliationAction::RequeuePreAcceptance,
                    ReceiverReconciliationReason::PreAcceptanceTimeout,
                    job.id(),
                    job.token(),
                    cleanup_instance,
                    cleanup_session_id,
                )))
            }
            ReceiverRecoveryDecision::RecoverSameSession => {
                let cleanup_instance = job.observation_instance().map(str::to_owned);
                let cleanup_session_id = job.observation_session_id().map(str::to_owned);
                match attribute_exact_recovery_session(&transaction, &self.workspace_id, &job, now)?
                {
                    RecoverySessionAttribution::Bound => {}
                    RecoverySessionAttribution::FreshConflict => {
                        return terminalize(
                            transaction,
                            &self.workspace_id,
                            &candidate,
                            &job,
                            ReceiverReconciliationReason::NativeSessionUnavailable,
                            now,
                            false,
                        );
                    }
                    RecoverySessionAttribution::Absent => {
                        return terminalize_without_observed_cleanup(
                            transaction,
                            &self.workspace_id,
                            &candidate,
                            &job,
                            ReceiverReconciliationReason::NativeSessionUnavailable,
                            now,
                            false,
                        );
                    }
                }
                let absolute_work_expires_at_unix_ms =
                    job.absolute_work_expires_at_unix_ms().ok_or_else(|| {
                        anyhow::anyhow!("recoverable receiver job has no absolute deadline")
                    })?;
                let recovery_expires = to_i64(
                    receiver_recovery_expires_at(now_unix_ms, absolute_work_expires_at_unix_ms),
                    "receiver recovery expiry",
                )?;
                let sql = format!(
                    "UPDATE receiver_jobs
                     SET state = 'retrying', retry_at_unix_ms = ?5,
                         retry_from_state = ?3, last_error = ?6,
                         claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                         observation_instance = NULL, observation_session_id = NULL,
                         observation_revision = 0,
                         attempt_accepted_at_unix_ms = NULL,
                         attempt_progressing_at_unix_ms = NULL,
                         latest_progress_at_unix_ms = NULL,
                         launch_expires_at_unix_ms = NULL,
                         acceptance_expires_at_unix_ms = NULL,
                         progress_expires_at_unix_ms = NULL,
                         recovery_expires_at_unix_ms = ?7,
                         recovery_count = recovery_count + 1,
                         attempt_kind = 'recovery',
                         recovery_cleanup_instance = ?12,
                         recovery_cleanup_session_id = ?13,
                         updated_at_unix_ms = ?5
                     WHERE workspace_id = ?1 AND job_id = ?2 AND state = ?3
                       AND claim_owner IS ?4 AND claim_expires_at_unix_ms IS ?8
                       AND updated_at_unix_ms = ?9 AND attempt_kind = 'ordinary'
                       AND recovery_count = ?10
                       AND {EXACT_SNAPSHOT_SQL} = ?11"
                );
                let changed = transaction.execute(
                    &sql,
                    rusqlite::params![
                        self.workspace_id,
                        candidate.job_id.to_string(),
                        candidate.state.as_str(),
                        candidate.owner,
                        now,
                        ReceiverReconciliationReason::AcceptedStall.as_str(),
                        recovery_expires,
                        candidate.claim_expires_at_unix_ms,
                        candidate.updated_at_unix_ms,
                        i64::from(job.recovery_count()),
                        candidate.exact_snapshot,
                        cleanup_instance,
                        cleanup_session_id,
                    ],
                )?;
                if changed != 1 {
                    return Ok(None);
                }
                transaction.commit()?;
                self.log_receiver_summary(|summary| {
                    crate::logging::ReceiverLifecycleEvent::recovery(
                        crate::logging::ReceiverLifecyclePhase::Retrying,
                        summary.recovery_attempt().unwrap_or(0),
                        summary.recovery_limit(),
                        crate::logging::ReceiverLifecycleReason::AcceptedStall,
                    )
                });
                Ok(Some(ReceiverReconciliationEffect::new(
                    ReceiverReconciliationAction::ScheduleRecovery,
                    ReceiverReconciliationReason::AcceptedStall,
                    job.id(),
                    job.token(),
                    cleanup_instance,
                    cleanup_session_id,
                )))
            }
            decision @ (ReceiverRecoveryDecision::TerminalFailure
            | ReceiverRecoveryDecision::IncompleteLegacyCompletion) => {
                if matches!(
                    candidate.state,
                    ReceiverJobState::Accepted | ReceiverJobState::Processing
                ) {
                    match attribute_exact_recovery_session(
                        &transaction,
                        &self.workspace_id,
                        &job,
                        now,
                    )? {
                        RecoverySessionAttribution::Bound => {}
                        RecoverySessionAttribution::FreshConflict => {
                            return terminalize(
                                transaction,
                                &self.workspace_id,
                                &candidate,
                                &job,
                                ReceiverReconciliationReason::NativeSessionUnavailable,
                                now,
                                false,
                            );
                        }
                        RecoverySessionAttribution::Absent => {
                            return terminalize_without_observed_cleanup(
                                transaction,
                                &self.workspace_id,
                                &candidate,
                                &job,
                                ReceiverReconciliationReason::NativeSessionUnavailable,
                                now,
                                false,
                            );
                        }
                    }
                }
                let reason = terminal_reason(&job, now_unix_ms, decision);
                let consume_launch_attempt = decision == ReceiverRecoveryDecision::TerminalFailure
                    && job.attempt_kind() == crate::state::ReceiverAttemptKind::Ordinary
                    && matches!(
                        job.state(),
                        ReceiverJobState::Claimed
                            | ReceiverJobState::Launching
                            | ReceiverJobState::Launched
                    );
                terminalize(
                    transaction,
                    &self.workspace_id,
                    &candidate,
                    &job,
                    reason,
                    now,
                    consume_launch_attempt,
                )
            }
        }
    }

    /// Terminalize one exact claimed recovery that cannot safely resume.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid owner, malformed durable state, an
    /// unrepresentable timestamp, or a database failure.
    pub fn fail_receiver_recovery_resume(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
    ) -> Result<Option<ReceiverReconciliationEffect>> {
        self.fail_receiver_recovery_with_reason(
            job_id,
            owner,
            now_unix_ms,
            ReceiverReconciliationReason::NativeSessionUnavailable,
            false,
        )
    }

    /// Terminalize one exact claimed recovery after a bounded launch failure.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid owner, malformed durable state, an
    /// unrepresentable timestamp, or a database failure.
    pub fn fail_receiver_recovery_attempt(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
        failure: ReceiverRecoveryFailure,
    ) -> Result<Option<ReceiverReconciliationEffect>> {
        self.fail_receiver_recovery_with_reason(
            job_id,
            owner,
            now_unix_ms,
            failure.reason(),
            failure == ReceiverRecoveryFailure::Shutdown,
        )
    }

    fn fail_receiver_recovery_with_reason(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
        reason: ReceiverReconciliationReason,
        allow_launched: bool,
    ) -> Result<Option<ReceiverReconciliationEffect>> {
        let owner = validated_owner(owner)?;
        let now = to_i64(now_unix_ms, "receiver recovery failure time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let Some(candidate) = candidate_for_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(None);
        };
        let Some(job) = load_receiver_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(None);
        };
        let eligible_state = matches!(
            candidate.state,
            ReceiverJobState::Claimed | ReceiverJobState::Launching
        ) || (allow_launched && candidate.state == ReceiverJobState::Launched);
        if !eligible_state
            || candidate.owner.as_deref() != Some(owner)
            || candidate
                .claim_expires_at_unix_ms
                .is_none_or(|expires_at| expires_at <= now)
            || job.attempt_kind() != crate::state::ReceiverAttemptKind::Recovery
        {
            return Ok(None);
        }
        if candidate.state == ReceiverJobState::Launched {
            return terminalize_launched_recovery(
                transaction,
                &self.workspace_id,
                &candidate,
                &job,
                reason,
                now,
            );
        }
        terminalize(
            transaction,
            &self.workspace_id,
            &candidate,
            &job,
            reason,
            now,
            false,
        )
    }
}
