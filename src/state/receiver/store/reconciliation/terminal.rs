use anyhow::Result;

use super::support::{EXACT_SNAPSHOT_SQL, ReconciliationCandidate, release_registration};
use crate::state::{
    ReceiverAttemptKind, ReceiverJob, ReceiverJobState, ReceiverReconciliationAction,
    ReceiverReconciliationEffect, ReceiverReconciliationReason, ReceiverRecoveryDecision,
};

pub(super) fn terminal_reason(
    job: &ReceiverJob,
    now_unix_ms: u64,
    decision: ReceiverRecoveryDecision,
) -> ReceiverReconciliationReason {
    if decision == ReceiverRecoveryDecision::IncompleteLegacyCompletion {
        return ReceiverReconciliationReason::IncompleteLegacyCompletion;
    }
    if job
        .absolute_work_expires_at_unix_ms()
        .is_some_and(|expiry| now_unix_ms >= expiry)
    {
        return ReceiverReconciliationReason::AbsoluteWorkExpired;
    }
    if job
        .recovery_expires_at_unix_ms()
        .is_some_and(|expiry| now_unix_ms >= expiry)
    {
        return ReceiverReconciliationReason::RecoveryExpired;
    }
    if matches!(
        job.state(),
        ReceiverJobState::Claimed | ReceiverJobState::Launching | ReceiverJobState::Launched
    ) && job.attempt_kind() == ReceiverAttemptKind::Ordinary
    {
        ReceiverReconciliationReason::PreAcceptanceExhausted
    } else {
        ReceiverReconciliationReason::RecoveryExhausted
    }
}

pub(super) fn terminalize(
    transaction: rusqlite::Transaction<'_>,
    workspace_id: &str,
    candidate: &ReconciliationCandidate,
    job: &ReceiverJob,
    reason: ReceiverReconciliationReason,
    now: i64,
    consume_launch_attempt: bool,
) -> Result<Option<ReceiverReconciliationEffect>> {
    let cleanup_instance = job
        .observation_instance()
        .map(str::to_owned)
        .or_else(|| candidate.owner.clone());
    let cleanup_session_id = job.observation_session_id().map(str::to_owned);
    let sql = format!(
        "UPDATE receiver_jobs
         SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_count = retry_count + ?5, retry_at_unix_ms = NULL,
             retry_from_state = NULL, last_error = ?6,
             observation_instance = NULL, observation_session_id = NULL,
             observation_revision = 0, attempt_accepted_at_unix_ms = NULL,
             attempt_progressing_at_unix_ms = NULL,
             latest_progress_at_unix_ms = NULL,
             launch_expires_at_unix_ms = NULL,
             acceptance_expires_at_unix_ms = NULL,
             progress_expires_at_unix_ms = NULL,
             pending_unavailable_notice = 1, updated_at_unix_ms = ?7
         WHERE workspace_id = ?1 AND job_id = ?2 AND state = ?3
           AND claim_owner IS ?4 AND {EXACT_SNAPSHOT_SQL} = ?8"
    );
    let changed = transaction.execute(
        &sql,
        rusqlite::params![
            workspace_id,
            candidate.job_id.to_string(),
            candidate.state.as_str(),
            candidate.owner,
            i64::from(consume_launch_attempt),
            reason.as_str(),
            now,
            candidate.exact_snapshot,
        ],
    )?;
    if changed != 1 {
        return Ok(None);
    }
    release_registration(
        &transaction,
        workspace_id,
        job.conversation_id(),
        cleanup_instance.as_deref(),
        now,
    )?;
    transaction.commit()?;
    Ok(Some(ReceiverReconciliationEffect::new(
        ReceiverReconciliationAction::TerminalFailure,
        reason,
        job.id(),
        job.token(),
        cleanup_instance,
        cleanup_session_id,
    )))
}
