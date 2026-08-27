use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use super::support::{EXACT_SNAPSHOT_SQL, candidate_for_job};
use crate::state::{
    Db, ReceiverAttemptKind, ReceiverJobId, ReceiverJobState, ReceiverJobToken,
    ReceiverReconciliationAction, ReceiverReconciliationEffect, ReceiverReconciliationReason,
};

use super::super::{load::load_receiver_job, to_i64};

impl Db {
    /// Return whether an exact cleanup registration is owned by a dead process.
    ///
    /// This is the narrow durable proof used after a TUI restart, when the tab
    /// that owned the receiver run is no longer present in local memory.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed cleanup identity or a database failure.
    pub fn receiver_cleanup_registration_is_stale(
        &self,
        effect: &ReceiverReconciliationEffect,
    ) -> Result<bool> {
        let (Some(instance), Some(session_id)) =
            (effect.cleanup_instance(), effect.cleanup_session_id())
        else {
            return Ok(false);
        };
        if instance.trim().is_empty() || session_id.trim().is_empty() {
            return Ok(false);
        }
        let locked_pid = self
            .conn
            .query_row(
                "SELECT session.locked_pid
                 FROM receiver_jobs AS job
                 JOIN receiver_conversations AS conversation
                   ON conversation.workspace_id = job.workspace_id
                  AND conversation.conversation_id = job.conversation_id
                  AND conversation.channel = job.channel
                 JOIN receiver_session_registrations AS registration
                   ON registration.workspace_id = conversation.workspace_id
                  AND registration.conversation_id = conversation.conversation_id
                  AND registration.actor_id = conversation.user_id
                  AND registration.channel = conversation.channel
                 JOIN brain_sessions AS session
                   ON session.workspace_id = registration.workspace_id
                  AND session.brain_instance_id = registration.brain_instance_id
                  AND session.agent_kind = registration.agent_kind
                  AND session.actor_id = registration.actor_id
                  AND session.channel = registration.channel
                  AND session.agent_session_id = ?5
                 WHERE job.workspace_id = ?1 AND job.job_id = ?2
                   AND job.job_token = ?3 AND job.state IN ('retrying', 'failed')
                   AND (job.state = 'failed' OR job.attempt_kind = 'recovery')
                   AND job.claim_owner IS NULL
                   AND job.claim_expires_at_unix_ms IS NULL
                   AND job.recovery_cleanup_instance = ?4
                   AND job.recovery_cleanup_session_id = ?5
                   AND registration.brain_instance_id = ?4
                   AND COALESCE(registration.actual_session_id,
                                registration.registered_session_id) = ?5
                   AND (
                     (conversation.agent_kind = registration.agent_kind
                      AND conversation.agent_session_id = session.agent_session_id)
                     OR (
                       job.state = 'failed' AND job.attempt_kind = 'ordinary'
                       AND conversation.agent_kind IS NULL
                       AND conversation.agent_session_id IS NULL
                       AND registration.actual_session_id IS NULL
                       AND registration.registered_session_id = ?5
                       AND session.source = 'fresh'
                     )
                   )",
                rusqlite::params![
                    self.workspace_id,
                    effect.job_id().to_string(),
                    effect.token().to_string(),
                    instance,
                    session_id,
                ],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();
        let Some(pid) = locked_pid.and_then(|pid| i32::try_from(pid).ok()) else {
            return Ok(false);
        };
        Ok(pid > 0 && !(self.pid_alive)(pid))
    }

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
            || (candidate.state == ReceiverJobState::Retrying
                && job.attempt_kind() != ReceiverAttemptKind::Recovery)
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
               AND state = ?8
               AND (state = 'failed' OR attempt_kind = 'recovery')
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
               AND (state = 'failed' OR attempt_kind = 'recovery')
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
             AND (conversation.agent_kind IS NULL
                  OR conversation.agent_kind = registration.agent_kind)
             AND (conversation.agent_session_id IS NULL
                  OR conversation.agent_session_id = session.agent_session_id)
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
