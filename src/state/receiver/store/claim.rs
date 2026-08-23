use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{Db, ReceiverClaim, ReceiverJobId, ReceiverJobState};

impl Db {
    /// Claim the oldest ready job without removing it from durable state.
    pub fn claim_next_receiver_job(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverClaim>> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver claim expiry must follow claim time"
        );
        let now = to_i64(now_unix_ms, "receiver claim time")?;
        let expires = to_i64(expires_at_unix_ms, "receiver claim expiry")?;
        let candidate = self
            .conn
            .query_row(
                "SELECT job_id FROM receiver_jobs
                 WHERE workspace_id = ?1
                   AND (
                     state = 'queued'
                     OR (state = 'retrying' AND retry_at_unix_ms <= ?2)
                     OR (
                       state NOT IN ('failed', 'done')
                       AND claim_owner IS NOT NULL
                       AND claim_expires_at_unix_ms <= ?2
                     )
                   )
                   AND (claim_owner IS NULL OR claim_expires_at_unix_ms <= ?2)
                   AND NOT EXISTS (
                     SELECT 1 FROM receiver_jobs AS live
                     WHERE live.workspace_id = ?1
                       AND live.claim_owner IS NOT NULL
                       AND live.claim_expires_at_unix_ms > ?2
                       AND live.state NOT IN ('failed', 'done')
                   )
                 ORDER BY received_at_unix_ms, job_id
                 LIMIT 1",
                rusqlite::params![self.workspace_id, now],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        let changed = self.conn.execute(
            "UPDATE receiver_jobs
             SET state = CASE
                   WHEN state = 'queued' THEN 'claimed'
                   ELSE state
                 END,
                 claim_owner = ?3, claim_expires_at_unix_ms = ?4,
                 retry_at_unix_ms = CASE
                   WHEN state = 'retrying' THEN NULL
                   ELSE retry_at_unix_ms
                 END,
                 updated_at_unix_ms = ?2
             WHERE workspace_id = ?1 AND job_id = ?5
               AND (
                 state = 'queued'
                 OR (state = 'retrying' AND retry_at_unix_ms <= ?2)
                 OR (
                   state NOT IN ('failed', 'done')
                   AND claim_owner IS NOT NULL
                   AND claim_expires_at_unix_ms <= ?2
                 )
               )
               AND (claim_owner IS NULL OR claim_expires_at_unix_ms <= ?2)
               AND NOT EXISTS (
                 SELECT 1 FROM receiver_jobs AS live
                 WHERE live.workspace_id = ?1
                   AND live.claim_owner IS NOT NULL
                   AND live.claim_expires_at_unix_ms > ?2
                   AND live.state NOT IN ('failed', 'done')
               )",
            rusqlite::params![self.workspace_id, now, owner, expires, candidate],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        Ok(Some(ReceiverClaim::new(
            ReceiverJobId::parse(&candidate)?,
            owner.to_owned(),
            expires_at_unix_ms,
        )))
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
        let owner = validated_owner(owner)?;
        let observed = to_i64(observed_at_unix_ms, "receiver transition time")?;
        let terminal = matches!(next, ReceiverJobState::Failed | ReceiverJobState::Done);
        let changed = if terminal {
            self.conn.execute(
                "UPDATE receiver_jobs
                 SET state = ?5, claim_owner = NULL, claim_expires_at_unix_ms = NULL,
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
                "UPDATE receiver_jobs SET state = ?5, updated_at_unix_ms = ?4
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
                 retry_at_unix_ms = ?6, last_error = ?7,
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
