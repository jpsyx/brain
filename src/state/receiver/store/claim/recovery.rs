//! Exact compare-and-swap claim for one accepted same-session recovery.

use anyhow::Result;

use super::{live::has_live_receiver_claim, next::commit_loaded_claim};
use crate::state::{
    Db, MAX_RECEIVER_RECOVERY_ATTEMPTS, ReceiverJobId, ReceiverRecoveryDecision, ReceiverRunClaim,
    decide_receiver_recovery, receiver_launch_expires_at, receiver_recovery_expires_at,
};

use super::super::{load::load_receiver_job, to_i64, validated_owner};

impl Db {
    /// Atomically claim one exactly stalled accepted job as its sole recovery attempt.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid owner or claim interval, an unrepresentable
    /// timestamp, malformed durable state, or a database failure.
    pub fn claim_receiver_recovery_run(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverRunClaim>> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver claim expiry must follow claim time"
        );
        let now = to_i64(now_unix_ms, "receiver recovery claim time")?;
        let expires = to_i64(expires_at_unix_ms, "receiver recovery claim expiry")?;
        let launch_expires = to_i64(
            receiver_launch_expires_at(now_unix_ms),
            "receiver recovery launch expiry",
        )?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if has_live_receiver_claim(&transaction, &self.workspace_id, now)? {
            return Ok(None);
        }
        let Some(job) = load_receiver_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(None);
        };
        if decide_receiver_recovery(job.recovery_snapshot(now_unix_ms))
            != ReceiverRecoveryDecision::RecoverSameSession
        {
            return Ok(None);
        }
        let Some(absolute_work_expires_at_unix_ms) = job.absolute_work_expires_at_unix_ms() else {
            return Ok(None);
        };
        let recovery_expires = to_i64(
            receiver_recovery_expires_at(now_unix_ms, absolute_work_expires_at_unix_ms),
            "receiver recovery expiry",
        )?;
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'claimed', claim_owner = ?3, claim_expires_at_unix_ms = ?4,
                 retry_at_unix_ms = NULL, retry_from_state = NULL, last_error = NULL,
                 observation_instance = NULL, observation_session_id = NULL,
                 observation_revision = 0, attempt_accepted_at_unix_ms = NULL,
                 attempt_progressing_at_unix_ms = NULL,
                 latest_progress_at_unix_ms = NULL,
                 launch_expires_at_unix_ms = ?5,
                 acceptance_expires_at_unix_ms = NULL,
                 progress_expires_at_unix_ms = NULL,
                 recovery_expires_at_unix_ms = ?6,
                 recovery_count = recovery_count + 1, attempt_kind = 'recovery',
                 pending_unavailable_notice = 0, updated_at_unix_ms = ?2
             WHERE workspace_id = ?1 AND job_id = ?7
               AND state = ?8 AND attempt_kind = 'ordinary'
               AND recovery_count = ?9 AND recovery_count < ?10
               AND claim_owner IS NOT NULL AND claim_expires_at_unix_ms <= ?2
               AND observation_revision = ?11
               AND progress_expires_at_unix_ms IS ?12
               AND recovery_expires_at_unix_ms IS ?13
               AND absolute_work_expires_at_unix_ms = ?14
               AND observation_instance IS ?15
               AND observation_session_id IS ?16
               AND attempt_accepted_at_unix_ms IS ?17
               AND attempt_progressing_at_unix_ms IS ?18",
            rusqlite::params![
                self.workspace_id,
                now,
                owner,
                expires,
                launch_expires,
                recovery_expires,
                job_id.to_string(),
                job.state().as_str(),
                i64::from(job.recovery_count()),
                i64::from(MAX_RECEIVER_RECOVERY_ATTEMPTS),
                to_i64(job.observation_revision(), "receiver observation revision")?,
                job.progress_expires_at_unix_ms()
                    .map(|value| to_i64(value, "receiver progress expiry"))
                    .transpose()?,
                job.recovery_expires_at_unix_ms()
                    .map(|value| to_i64(value, "receiver prior recovery expiry"))
                    .transpose()?,
                to_i64(
                    absolute_work_expires_at_unix_ms,
                    "receiver absolute-work expiry",
                )?,
                job.observation_instance(),
                job.observation_session_id(),
                job.attempt_accepted_at_unix_ms()
                    .map(|value| to_i64(value, "receiver attempt acceptance"))
                    .transpose()?,
                job.attempt_progressing_at_unix_ms()
                    .map(|value| to_i64(value, "receiver attempt progress"))
                    .transpose()?,
            ],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        commit_loaded_claim(
            transaction,
            &self.workspace_id,
            &job_id.to_string(),
            owner,
            expires_at_unix_ms,
            "claimed recovery",
        )
    }
}
