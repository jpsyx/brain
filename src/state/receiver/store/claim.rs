use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{
    Db, MAX_RECEIVER_LAUNCH_ATTEMPTS, ReceiverJobId, ReceiverJobState, ReceiverLaunchFailure,
    ReceiverLaunchRetryOutcome,
};

mod next;
mod recovery;
mod restart;

impl Db {
    /// Atomically move one exact live launch-eligible owner to `launching`.
    pub fn prepare_receiver_job_launch(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        let observed = to_i64(observed_at_unix_ms, "receiver launch preparation time")?;
        Ok(self.conn.execute(
            "UPDATE receiver_jobs
             SET state = 'launching', retry_at_unix_ms = NULL,
                 retry_from_state = NULL, updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
               AND claim_expires_at_unix_ms > ?4
               AND (
                 state = 'claimed'
                 OR (
                   state = 'retrying' AND retry_at_unix_ms <= ?4
                   AND retry_from_state IN ('claimed', 'launching')
                 )
               )",
            rusqlite::params![self.workspace_id, job_id.to_string(), owner, observed],
        )? == 1)
    }

    /// Record one bounded pre-acceptance launch retry for the exact live owner.
    pub fn record_receiver_launch_retry(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
        retry_at_unix_ms: u64,
        failure: ReceiverLaunchFailure,
    ) -> Result<Option<ReceiverLaunchRetryOutcome>> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            retry_at_unix_ms > observed_at_unix_ms,
            "receiver launch retry time must follow observation time"
        );
        let observed = to_i64(observed_at_unix_ms, "receiver launch failure time")?;
        let retry_at = to_i64(retry_at_unix_ms, "receiver launch retry time")?;
        let expected = failure.expected_state();
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let retry_count = transaction
            .query_row(
                "SELECT retry_count FROM receiver_jobs
                 WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
                   AND claim_expires_at_unix_ms > ?4
                   AND (
                     state = ?5
                     OR (
                       ?5 = 'claimed' AND state = 'retrying'
                       AND retry_at_unix_ms <= ?4
                       AND retry_from_state IN ('claimed', 'launching')
                     )
                   )",
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    owner,
                    observed,
                    expected.as_str(),
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(retry_count) = retry_count else {
            return Ok(None);
        };
        let next_count = retry_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("receiver launch retry count is exhausted"))?;
        let exhausted = next_count >= i64::from(MAX_RECEIVER_LAUNCH_ATTEMPTS);
        let next_state = if exhausted { "failed" } else { "retrying" };
        let retry_at = (!exhausted).then_some(retry_at);
        let retry_from = (!exhausted).then_some(expected.as_str());
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = ?6, retry_count = ?7, retry_at_unix_ms = ?8,
                 retry_from_state = ?9, last_error = ?10,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
               AND claim_expires_at_unix_ms > ?4
               AND (
                 state = ?5
                 OR (
                   ?5 = 'claimed' AND state = 'retrying'
                   AND retry_at_unix_ms <= ?4
                   AND retry_from_state IN ('claimed', 'launching')
                 )
               )
               AND retry_count = ?11",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                owner,
                observed,
                expected.as_str(),
                next_state,
                next_count,
                retry_at,
                retry_from,
                failure.as_str(),
                retry_count,
            ],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(if exhausted {
            ReceiverLaunchRetryOutcome::Exhausted
        } else {
            ReceiverLaunchRetryOutcome::Scheduled
        }))
    }

    /// Renew an unexpired claim only for its exact owner.
    pub fn renew_receiver_claim(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver claim expiry must follow renewal time"
        );
        Ok(self.conn.execute(
            "UPDATE receiver_jobs
             SET claim_expires_at_unix_ms = ?5, updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
               AND claim_expires_at_unix_ms > ?4
               AND state NOT IN ('failed', 'done')",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                owner,
                to_i64(now_unix_ms, "receiver claim renewal time")?,
                to_i64(expires_at_unix_ms, "receiver claim renewal expiry")?,
            ],
        )? == 1)
    }

    /// Advance one job only when the expected state and live claim owner match.
    pub fn transition_receiver_job(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        expected: ReceiverJobState,
        next: ReceiverJobState,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        anyhow::ensure!(
            expected.can_transition_to(next),
            "invalid receiver job transition {} -> {}",
            expected.as_str(),
            next.as_str()
        );
        if expected == ReceiverJobState::Retrying && next == ReceiverJobState::Launching {
            return self.prepare_receiver_job_launch(job_id, owner, observed_at_unix_ms);
        }
        let owner = validated_owner(owner)?;
        let observed = to_i64(observed_at_unix_ms, "receiver transition time")?;
        let terminal = matches!(next, ReceiverJobState::Failed | ReceiverJobState::Done);
        let changed = if terminal {
            self.conn.execute(
                "UPDATE receiver_jobs
                 SET state = ?5, claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                     retry_at_unix_ms = NULL, retry_from_state = NULL,
                     updated_at_unix_ms = ?4
                 WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
                   AND claim_expires_at_unix_ms > ?4 AND state = ?6",
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    owner,
                    observed,
                    next.as_str(),
                    expected.as_str(),
                ],
            )?
        } else {
            self.conn.execute(
                "UPDATE receiver_jobs SET state = ?5,
                     retry_at_unix_ms = CASE WHEN state = 'retrying' THEN NULL ELSE retry_at_unix_ms END,
                     retry_from_state = CASE WHEN state = 'retrying' THEN NULL ELSE retry_from_state END,
                     updated_at_unix_ms = ?4
                 WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
                   AND claim_expires_at_unix_ms > ?4 AND state = ?6",
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    owner,
                    observed,
                    next.as_str(),
                    expected.as_str(),
                ],
            )?
        };
        Ok(changed == 1)
    }

    /// Persist one bounded retry decision and release its claim until due.
    pub fn record_receiver_retry(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        expected: ReceiverJobState,
        observed_at_unix_ms: u64,
        retry_at_unix_ms: u64,
        error_summary: &str,
    ) -> Result<bool> {
        anyhow::ensure!(
            expected.can_transition_to(ReceiverJobState::Retrying),
            "invalid receiver retry transition from {}",
            expected.as_str()
        );
        let owner = validated_owner(owner)?;
        let error_summary = error_summary.trim();
        anyhow::ensure!(
            !error_summary.is_empty(),
            "receiver retry error cannot be blank"
        );
        anyhow::ensure!(
            retry_at_unix_ms > observed_at_unix_ms,
            "receiver retry time must follow observation time"
        );
        let observed = to_i64(observed_at_unix_ms, "receiver retry observation time")?;
        let retry_count = self
            .conn
            .query_row(
                "SELECT retry_count FROM receiver_jobs
                 WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
                   AND claim_expires_at_unix_ms > ?4 AND state = ?5",
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    owner,
                    observed,
                    expected.as_str(),
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(retry_count) = retry_count else {
            return Ok(false);
        };
        anyhow::ensure!(
            retry_count < i64::from(u32::MAX),
            "receiver retry count is exhausted"
        );
        Ok(self.conn.execute(
            "UPDATE receiver_jobs
             SET state = 'retrying', retry_count = retry_count + 1,
                 retry_at_unix_ms = ?6, retry_from_state = ?5, last_error = ?7,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
               AND claim_expires_at_unix_ms > ?4 AND state = ?5
               AND retry_count < ?8",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                owner,
                observed,
                expected.as_str(),
                to_i64(retry_at_unix_ms, "receiver retry time")?,
                error_summary,
                i64::from(u32::MAX),
            ],
        )? == 1)
    }
}
