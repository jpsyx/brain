//! Exact compare-and-swap claim for one already-persisted same-session recovery.

use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{live::has_live_receiver_claim, next::commit_loaded_claim};
use crate::state::{
    Db, ReceiverAttemptKind, ReceiverJobId, ReceiverJobState, ReceiverRunClaim,
    receiver_launch_expires_at,
};

use super::super::{load::load_receiver_job, to_i64, validated_owner};

impl Db {
    /// Find and claim the oldest due recovery persisted by reconciliation.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid owner or claim interval, an unrepresentable
    /// timestamp, malformed durable state, or a database failure.
    pub fn claim_next_receiver_recovery_run(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverRunClaim>> {
        validated_owner(owner)?;
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver claim expiry must follow claim time"
        );
        let now = to_i64(now_unix_ms, "receiver recovery discovery time")?;
        let job_id = self
            .conn
            .query_row(
                "SELECT job_id FROM receiver_jobs
                 WHERE workspace_id = ?1 AND state = 'retrying'
                   AND attempt_kind = 'recovery' AND claim_owner IS NULL
                   AND claim_expires_at_unix_ms IS NULL
                   AND retry_at_unix_ms <= ?2
                   AND recovery_expires_at_unix_ms > ?2
                   AND absolute_work_expires_at_unix_ms > ?2
                 ORDER BY received_at_unix_ms, job_id
                 LIMIT 1",
                rusqlite::params![self.workspace_id, now],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(job_id) = job_id else {
            return Ok(None);
        };
        self.claim_receiver_recovery_run(
            ReceiverJobId::parse(&job_id)?,
            owner,
            now_unix_ms,
            expires_at_unix_ms,
        )
    }

    /// Atomically claim one due recovery that reconciliation already persisted.
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
        if job.state() != ReceiverJobState::Retrying
            || job.attempt_kind() != ReceiverAttemptKind::Recovery
            || job
                .retry_at_unix_ms()
                .is_none_or(|retry_at| retry_at > now_unix_ms)
            || job
                .recovery_expires_at_unix_ms()
                .is_none_or(|expires_at| expires_at <= now_unix_ms)
            || job
                .absolute_work_expires_at_unix_ms()
                .is_none_or(|expires_at| expires_at <= now_unix_ms)
        {
            return Ok(None);
        }
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'claimed', claim_owner = ?3, claim_expires_at_unix_ms = ?4,
                 retry_at_unix_ms = NULL, retry_from_state = NULL, last_error = NULL,
                 launch_expires_at_unix_ms = ?5,
                 pending_unavailable_notice = 0, updated_at_unix_ms = ?2
             WHERE workspace_id = ?1 AND job_id = ?6 AND job_token = ?7
               AND state = 'retrying' AND attempt_kind = 'recovery'
               AND claim_owner IS NULL AND claim_expires_at_unix_ms IS NULL
               AND retry_at_unix_ms = ?8 AND retry_at_unix_ms <= ?2
               AND recovery_count = ?9
               AND recovery_expires_at_unix_ms = ?10
               AND recovery_expires_at_unix_ms > ?2
               AND absolute_work_expires_at_unix_ms = ?11
               AND absolute_work_expires_at_unix_ms > ?2
               AND observation_instance IS NULL
               AND observation_session_id IS NULL
               AND observation_revision = 0
               AND attempt_accepted_at_unix_ms IS NULL
               AND attempt_progressing_at_unix_ms IS NULL",
            rusqlite::params![
                self.workspace_id,
                now,
                owner,
                expires,
                launch_expires,
                job_id.to_string(),
                job.token().to_string(),
                job.retry_at_unix_ms()
                    .map(|value| to_i64(value, "receiver recovery due time"))
                    .transpose()?,
                i64::from(job.recovery_count()),
                job.recovery_expires_at_unix_ms()
                    .map(|value| to_i64(value, "receiver recovery expiry"))
                    .transpose()?,
                job.absolute_work_expires_at_unix_ms()
                    .map(|value| to_i64(value, "receiver absolute-work expiry"))
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
