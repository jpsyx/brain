use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use super::support::{EXACT_SNAPSHOT_SQL, candidate_for_job};
use crate::state::{
    Db, ReceiverAttemptKind, ReceiverJobId, ReceiverJobState, ReceiverJobToken,
    ReceiverReconciliationAction, ReceiverReconciliationEffect, ReceiverReconciliationReason,
};

use super::super::{load::load_receiver_job, to_i64};

impl Db {
    /// Acknowledge exact local cleanup for a due or terminal persisted recovery.
    ///
    /// # Errors
    ///
    /// Returns an error for blank cleanup identity, an unrepresentable timestamp,
    /// malformed durable state, or a database failure.
    pub fn acknowledge_receiver_recovery_cleanup(
        &self,
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        instance: &str,
        session_id: &str,
        now_unix_ms: u64,
    ) -> Result<bool> {
        anyhow::ensure!(
            !instance.trim().is_empty(),
            "receiver cleanup instance cannot be blank"
        );
        anyhow::ensure!(
            !session_id.trim().is_empty(),
            "receiver cleanup session cannot be blank"
        );
        let now = to_i64(now_unix_ms, "receiver cleanup acknowledgement time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let Some(candidate) = candidate_for_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(false);
        };
        let Some(job) = load_receiver_job(&transaction, &self.workspace_id, job_id)? else {
            return Ok(false);
        };
        if job.token() != token
            || !matches!(
                candidate.state,
                ReceiverJobState::Retrying | ReceiverJobState::Failed
            )
            || candidate.owner.is_some()
            || job.attempt_kind() != ReceiverAttemptKind::Recovery
            || (candidate.state == ReceiverJobState::Failed && !job.pending_unavailable_notice())
            || job.recovery_cleanup_instance() != Some(instance)
            || job.recovery_cleanup_session_id() != Some(session_id)
        {
            return Ok(false);
        }
        if !release_exact_cleanup_registration(
            &transaction,
            &self.workspace_id,
            &job,
            instance,
            session_id,
            now,
        )? {
            return Ok(false);
        }
        let sql = format!(
            "UPDATE receiver_jobs
             SET recovery_cleanup_instance = NULL,
                 recovery_cleanup_session_id = NULL,
                 updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND state = ?8 AND attempt_kind = 'recovery'
               AND claim_owner IS NULL AND claim_expires_at_unix_ms IS NULL
               AND recovery_cleanup_instance = ?4
               AND recovery_cleanup_session_id = ?6
               AND {EXACT_SNAPSHOT_SQL} = ?7"
        );
        if transaction.execute(
            &sql,
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                token.to_string(),
                instance,
                now,
                session_id,
                candidate.exact_snapshot,
                candidate.state.as_str(),
            ],
        )? != 1
        {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}

pub(super) fn pending_cleanup_effect(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
) -> Result<Option<ReceiverReconciliationEffect>> {
    let job_id = transaction
        .query_row(
            "SELECT job_id FROM receiver_jobs
             WHERE workspace_id = ?1
               AND state IN ('retrying', 'failed')
               AND attempt_kind = 'recovery'
               AND claim_owner IS NULL AND claim_expires_at_unix_ms IS NULL
               AND recovery_cleanup_instance IS NOT NULL
               AND recovery_cleanup_session_id IS NOT NULL
             ORDER BY received_at_unix_ms, job_id
             LIMIT 1",
            [workspace_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(job_id) = job_id else {
        return Ok(None);
    };
    let job_id = ReceiverJobId::parse(&job_id)?;
    let job = load_receiver_job(transaction, workspace_id, job_id)?
        .context("pending receiver cleanup job disappeared")?;
    let reason = ReceiverReconciliationReason::parse(
        job.last_error()
            .context("pending receiver cleanup has no stable reason")?,
    )
    .context("pending receiver cleanup has an unknown stable reason")?;
    let action = if job.state() == ReceiverJobState::Failed {
        ReceiverReconciliationAction::TerminalFailure
    } else {
        ReceiverReconciliationAction::ScheduleRecovery
    };
    Ok(Some(ReceiverReconciliationEffect::new(
        action,
        reason,
        job.id(),
        job.token(),
        job.recovery_cleanup_instance().map(str::to_owned),
        job.recovery_cleanup_session_id().map(str::to_owned),
    )))
}

fn release_exact_cleanup_registration(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job: &crate::state::ReceiverJob,
    instance: &str,
    session_id: &str,
    now: i64,
) -> Result<bool> {
    let exact_registration = transaction.query_row(
        "SELECT EXISTS (
           SELECT 1 FROM receiver_session_registrations AS registration
           JOIN brain_sessions AS session
             ON session.workspace_id = registration.workspace_id
            AND session.brain_instance_id = registration.brain_instance_id
            AND session.agent_kind = registration.agent_kind
            AND session.actor_id = registration.actor_id
            AND session.channel = registration.channel
           JOIN receiver_conversations AS conversation
             ON conversation.workspace_id = registration.workspace_id
            AND conversation.conversation_id = registration.conversation_id
            AND conversation.user_id = registration.actor_id
            AND conversation.channel = registration.channel
            AND conversation.agent_kind = registration.agent_kind
            AND conversation.agent_session_id = session.agent_session_id
           JOIN receiver_jobs AS job
             ON job.workspace_id = conversation.workspace_id
            AND job.conversation_id = conversation.conversation_id
            AND job.channel = conversation.channel
           WHERE registration.workspace_id = ?1
             AND registration.conversation_id = ?2
             AND registration.brain_instance_id = ?3
             AND session.agent_session_id = ?4
             AND COALESCE(registration.actual_session_id,
                          registration.registered_session_id) = ?4
             AND job.job_id = ?5 AND job.job_token = ?6
         )",
        rusqlite::params![
            workspace_id,
            job.conversation_id().to_string(),
            instance,
            session_id,
            job.id().to_string(),
            job.token().to_string(),
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if !exact_registration {
        return Ok(false);
    }
    super::support::release_registration(
        transaction,
        workspace_id,
        job.conversation_id(),
        Some(instance),
        now,
    )?;
    Ok(true)
}
