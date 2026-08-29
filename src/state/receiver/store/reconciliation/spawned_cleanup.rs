use anyhow::Result;

use super::recovery_registration::exact_recovery_registration;
use super::support::{EXACT_SNAPSHOT_SQL, candidate_for_job};
use crate::state::{
    Db, ReceiverAttemptKind, ReceiverJobId, ReceiverJobState, ReceiverJobToken,
    ReceiverReconciliationAction, ReceiverReconciliationEffect, ReceiverReconciliationReason,
    ReceiverRecoveryCleanupOutcome, ReceiverSessionAttribution,
};

use super::super::{load::load_receiver_job, to_i64, validated_owner};

impl Db {
    /// Establish exact terminal cleanup after one spawned recovery has shut down.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid identity, timestamps, or durable store failures.
    #[allow(clippy::too_many_arguments)]
    pub fn establish_receiver_spawned_recovery_cleanup(
        &self,
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        original_owner: &str,
        registration: &ReceiverSessionAttribution,
        locked_pid: i32,
        now_unix_ms: u64,
    ) -> Result<ReceiverRecoveryCleanupOutcome> {
        let original_owner = validated_owner(original_owner)?;
        anyhow::ensure!(
            locked_pid > 0,
            "receiver recovery cleanup PID must be positive"
        );
        anyhow::ensure!(
            registration.scope().workspace_id().to_string() == self.workspace_id,
            "receiver recovery cleanup belongs to another workspace"
        );
        let now = to_i64(now_unix_ms, "receiver recovery cleanup time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let Some(candidate) = candidate_for_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        };
        let Some(job) = load_receiver_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        };
        if job.token() != token
            || job.attempt_kind() != ReceiverAttemptKind::Recovery
            || job.conversation_id() != registration.conversation_id()
            || registration.instance() != original_owner
            || registration.scope().actor() != &job.inbound().actor
        {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        }
        let Some(exact) = exact_recovery_registration(
            &transaction,
            &self.workspace_id,
            &job,
            registration.instance(),
        )?
        else {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        };
        if exact.agent_kind != registration.scope().agent_kind().as_str()
            || exact.session_id != registration.registered_session().as_str()
            || exact.locked_pid != locked_pid
        {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        }
        if matches!(
            candidate.state,
            ReceiverJobState::Retrying | ReceiverJobState::Failed
        ) && candidate.owner.is_none()
            && job.recovery_cleanup_instance() == Some(registration.instance())
            && job.recovery_cleanup_session_id() == Some(registration.registered_session().as_str())
        {
            let Some(reason) = job
                .last_error()
                .and_then(ReceiverReconciliationReason::parse)
            else {
                return Ok(ReceiverRecoveryCleanupOutcome::Changed);
            };
            return Ok(ReceiverRecoveryCleanupOutcome::Exact(cleanup_effect(
                &job,
                reason,
                registration,
            )));
        }
        if !matches!(
            candidate.state,
            ReceiverJobState::Claimed
                | ReceiverJobState::Launching
                | ReceiverJobState::Launched
                | ReceiverJobState::Accepted
                | ReceiverJobState::Processing
        ) || candidate.owner.as_deref() != Some(original_owner)
        {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        }
        if !super::terminal::insert_unavailable_notice(
            &transaction,
            &job,
            crate::state::ReceiverDeliveryState::CleanupGated,
            now,
        )? {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        }
        let sql = format!(
            "UPDATE receiver_jobs
             SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL, last_error = ?5,
                 observation_instance = NULL, observation_session_id = NULL,
                 observation_revision = 0, attempt_accepted_at_unix_ms = NULL,
                 attempt_progressing_at_unix_ms = NULL, latest_progress_at_unix_ms = NULL,
                 launch_expires_at_unix_ms = NULL, acceptance_expires_at_unix_ms = NULL,
                 progress_expires_at_unix_ms = NULL,
                 recovery_cleanup_instance = ?6, recovery_cleanup_session_id = ?7,
                 updated_at_unix_ms = ?8
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = ?4 AND attempt_kind = 'recovery'
               AND claim_owner = ?6 AND {EXACT_SNAPSHOT_SQL} = ?9"
        );
        if transaction.execute(
            &sql,
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                token.to_string(),
                candidate.state.as_str(),
                ReceiverReconciliationReason::RecoveryShutdown.as_str(),
                registration.instance(),
                registration.registered_session().as_str(),
                now,
                candidate.exact_snapshot,
            ],
        )? != 1
        {
            return Ok(ReceiverRecoveryCleanupOutcome::Changed);
        }
        transaction.commit()?;
        Ok(ReceiverRecoveryCleanupOutcome::Exact(cleanup_effect(
            &job,
            ReceiverReconciliationReason::RecoveryShutdown,
            registration,
        )))
    }
}

fn cleanup_effect(
    job: &crate::state::ReceiverJob,
    reason: ReceiverReconciliationReason,
    registration: &ReceiverSessionAttribution,
) -> ReceiverReconciliationEffect {
    ReceiverReconciliationEffect::new(
        ReceiverReconciliationAction::TerminalFailure,
        reason,
        job.id(),
        job.token(),
        Some(registration.instance().to_owned()),
        Some(registration.registered_session().as_str().to_owned()),
    )
}
