//! Finite exact writer lease for one persisted terminal unavailable notice.

use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{load::load_receiver_job, to_i64, validated_owner};
use crate::state::{
    Db, ReceiverJobId, ReceiverJobState, ReceiverJobToken, ReceiverUnavailableNoticeClaim,
};

impl Db {
    /// Claim the oldest pending terminal notice without participating in work FIFO ownership.
    pub fn claim_next_receiver_unavailable_notice(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverUnavailableNoticeClaim>> {
        let owner = validated_owner(owner)?.to_owned();
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver unavailable-notice expiry must follow claim time"
        );
        let now = to_i64(now_unix_ms, "receiver unavailable-notice claim time")?;
        let expires = to_i64(
            expires_at_unix_ms,
            "receiver unavailable-notice claim expiry",
        )?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let job_id = transaction
            .query_row(
                "SELECT job_id FROM receiver_jobs
                 WHERE workspace_id = ?1 AND state = 'failed'
                   AND pending_unavailable_notice = 1
                   AND (unavailable_notice_owner IS NULL
                        OR unavailable_notice_expires_at_unix_ms <= ?2)
                 ORDER BY received_at_unix_ms, job_id
                 LIMIT 1",
                rusqlite::params![self.workspace_id, now],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(job_id) = job_id else {
            return Ok(None);
        };
        let job_id = ReceiverJobId::parse(&job_id)?;
        let Some(job) = load_receiver_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(None);
        };
        if job.state() != ReceiverJobState::Failed || !job.pending_unavailable_notice() {
            return Ok(None);
        }
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET unavailable_notice_owner = ?4,
                 unavailable_notice_expires_at_unix_ms = ?5,
                 updated_at_unix_ms = ?2
             WHERE workspace_id = ?1 AND job_id = ?3 AND job_token = ?6
               AND state = 'failed' AND pending_unavailable_notice = 1
               AND (unavailable_notice_owner IS NULL
                    OR unavailable_notice_expires_at_unix_ms <= ?2)",
            rusqlite::params![
                self.workspace_id,
                now,
                job_id.to_string(),
                owner,
                expires,
                job.token().to_string(),
            ],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(ReceiverUnavailableNoticeClaim::new(
            &job,
            owner,
            expires_at_unix_ms,
        )))
    }

    /// Clear one pending notice only after its exact live writer queued it locally.
    pub fn acknowledge_receiver_unavailable_notice(
        &self,
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        owner: &str,
        now_unix_ms: u64,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        let now = to_i64(
            now_unix_ms,
            "receiver unavailable-notice acknowledgement time",
        )?;
        Ok(self.conn.execute(
            "UPDATE receiver_jobs
             SET pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL,
                 updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = 'failed' AND pending_unavailable_notice = 1
               AND unavailable_notice_owner = ?4
               AND unavailable_notice_expires_at_unix_ms > ?5",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                token.to_string(),
                owner,
                now,
            ],
        )? == 1)
    }
}
