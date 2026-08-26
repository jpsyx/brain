//! Atomic FIFO claim selection with post-spawn ambiguity fencing.

use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::restart::has_ready_restart;
use crate::state::{
    Db, ReceiverClaim, ReceiverJobId, ReceiverRunClaim, receiver_launch_expires_at,
};

use super::super::{
    load::{load_receiver_conversation, load_receiver_job},
    to_i64, validated_owner,
};

struct ClaimCandidate {
    job_id: String,
    state: String,
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
        let launch_expires = to_i64(
            receiver_launch_expires_at(now_unix_ms),
            "receiver launch expiry",
        )?;
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
        if matches!(
            candidate.state.as_str(),
            "launching" | "launched" | "accepted" | "processing"
        ) {
            return Ok(None);
        }
        if !replace_candidate_lease(
            &transaction,
            &self.workspace_id,
            &candidate.job_id,
            owner,
            now,
            expires,
            launch_expires,
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
            "SELECT job_id, state
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
    launch_expires: i64,
) -> Result<bool> {
    Ok(transaction.execute(
        "UPDATE receiver_jobs
         SET state = CASE WHEN state = 'queued' THEN 'claimed' ELSE state END,
             claim_owner = ?3, claim_expires_at_unix_ms = ?4,
             launch_expires_at_unix_ms = CASE
               WHEN state = 'queued'
                 OR (state = 'retrying' AND retry_from_state IN ('claimed', 'launching'))
               THEN ?6 ELSE launch_expires_at_unix_ms END,
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
        rusqlite::params![workspace_id, now, owner, expires, job_id, launch_expires],
    )? == 1)
}

pub(super) fn commit_loaded_claim(
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
