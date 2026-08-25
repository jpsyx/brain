//! Atomic FIFO claim selection and expired-launch normalization.

use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::recovery::{ExpiredLaunchingRecovery, recover_expired_launching};
use super::restart::has_ready_restart;
use crate::state::{Db, ReceiverClaim, ReceiverJobId, ReceiverJobState, ReceiverRunClaim};

use super::super::{
    load::{load_receiver_conversation, load_receiver_job},
    to_i64, validated_owner,
};

struct ClaimCandidate {
    job_id: String,
    state: String,
    conversation_id: String,
    owner: Option<String>,
    retry_count: i64,
}

impl Db {
    /// Claim the oldest ready job without removing it from durable state.
    pub fn claim_next_receiver_job(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverClaim>> {
        Ok(self
            .claim_next_receiver_run(owner, now_unix_ms, expires_at_unix_ms)?
            .map(|run| run.claim().clone()))
    }

    /// Atomically claim and load the oldest ready receiver run.
    pub fn claim_next_receiver_run(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<ReceiverRunClaim>> {
        let owner = validated_owner(owner)?;
        anyhow::ensure!(
            expires_at_unix_ms > now_unix_ms,
            "receiver claim expiry must follow claim time"
        );
        let now = to_i64(now_unix_ms, "receiver claim time")?;
        let expires = to_i64(expires_at_unix_ms, "receiver claim expiry")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if has_ready_restart(&transaction, &self.workspace_id)? {
            return Ok(None);
        }
        let candidate = oldest_ready_candidate(&transaction, &self.workspace_id, now)?;
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        if candidate.state == ReceiverJobState::Launching.as_str() {
            match recover_expired_launching(
                &transaction,
                &self.workspace_id,
                &candidate.job_id,
                &candidate.conversation_id,
                candidate.owner.as_deref(),
                candidate.retry_count,
                owner,
                now,
                expires,
            )? {
                ExpiredLaunchingRecovery::Retrying => {
                    return commit_loaded_claim(
                        transaction,
                        &self.workspace_id,
                        &candidate.job_id,
                        owner,
                        expires_at_unix_ms,
                        "recovered",
                    );
                }
                ExpiredLaunchingRecovery::Exhausted => {
                    transaction.commit()?;
                    return Ok(None);
                }
                ExpiredLaunchingRecovery::ChangedElsewhere => return Ok(None),
            }
        }
        if !replace_candidate_lease(
            &transaction,
            &self.workspace_id,
            &candidate.job_id,
            owner,
            now,
            expires,
        )? {
            return Ok(None);
        }
        commit_loaded_claim(
            transaction,
            &self.workspace_id,
            &candidate.job_id,
            owner,
            expires_at_unix_ms,
            "claimed",
        )
    }
}

fn oldest_ready_candidate(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<Option<ClaimCandidate>> {
    Ok(transaction
        .query_row(
            "SELECT job_id, state, conversation_id, claim_owner, retry_count
             FROM receiver_jobs
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
            rusqlite::params![workspace_id, now],
            |row| {
                Ok(ClaimCandidate {
                    job_id: row.get(0)?,
                    state: row.get(1)?,
                    conversation_id: row.get(2)?,
                    owner: row.get(3)?,
                    retry_count: row.get(4)?,
                })
            },
        )
        .optional()?)
}

fn replace_candidate_lease(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job_id: &str,
    owner: &str,
    now: i64,
    expires: i64,
) -> Result<bool> {
    Ok(transaction.execute(
        "UPDATE receiver_jobs
         SET state = CASE WHEN state = 'queued' THEN 'claimed' ELSE state END,
             claim_owner = ?3, claim_expires_at_unix_ms = ?4,
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
        rusqlite::params![workspace_id, now, owner, expires, job_id],
    )? == 1)
}

fn commit_loaded_claim(
    transaction: rusqlite::Transaction<'_>,
    workspace_id: &str,
    candidate: &str,
    owner: &str,
    expires_at_unix_ms: u64,
    action: &str,
) -> Result<Option<ReceiverRunClaim>> {
    let job_id = ReceiverJobId::parse(candidate)?;
    let claim = ReceiverClaim::new(job_id, owner.to_owned(), expires_at_unix_ms);
    let job = load_receiver_job(&transaction, workspace_id, job_id)?
        .ok_or_else(|| anyhow::anyhow!("{action} receiver job disappeared"))?;
    let conversation =
        load_receiver_conversation(&transaction, workspace_id, job.conversation_id())?
            .ok_or_else(|| anyhow::anyhow!("{action} receiver conversation disappeared"))?;
    transaction.commit()?;
    Ok(Some(ReceiverRunClaim::new(claim, job, conversation)))
}
