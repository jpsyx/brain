use anyhow::Result;
use rusqlite::OptionalExtension as _;

use crate::state::{ReceiverJob, ReceiverJobId, ReceiverJobToken, ReceiverReconciliationReason};

pub(super) struct Registration {
    agent_kind: String,
    actor_id: String,
    channel: String,
    registered_session_id: String,
    actual_session_id: Option<String>,
    pub(super) locked_pid: i64,
}

pub(super) fn registration(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    instance: &str,
    session_id: &str,
) -> Result<Option<Registration>> {
    connection
        .query_row(
            "SELECT registration.agent_kind, registration.actor_id,
                    registration.channel, registration.registered_session_id,
                    registration.actual_session_id, session.locked_pid
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
              AND session.locked_pid IS NOT NULL
              AND session.source = 'fresh'
             WHERE job.workspace_id = ?1 AND job.job_id = ?2
               AND job.job_token = ?3 AND job.state = 'failed'
               AND job.attempt_kind = 'ordinary'
               AND job.claim_owner IS NULL
               AND job.claim_expires_at_unix_ms IS NULL
               AND job.last_error = ?6
               AND job.recovery_cleanup_instance = ?4
               AND job.recovery_cleanup_session_id = ?5
               AND registration.brain_instance_id = ?4
               AND registration.registered_session_id != ?5
               AND conversation.agent_kind = registration.agent_kind
               AND conversation.agent_session_id IS NOT NULL
               AND conversation.agent_session_id != ?5
               AND conversation.agent_session_id != registration.registered_session_id
               AND (registration.actual_session_id IS NULL
                    OR registration.actual_session_id = conversation.agent_session_id)",
            rusqlite::params![
                workspace_id,
                job_id.to_string(),
                token.to_string(),
                instance,
                session_id,
                ReceiverReconciliationReason::NativeSessionUnavailable.as_str(),
            ],
            |row| {
                Ok(Registration {
                    agent_kind: row.get(0)?,
                    actor_id: row.get(1)?,
                    channel: row.get(2)?,
                    registered_session_id: row.get(3)?,
                    actual_session_id: row.get(4)?,
                    locked_pid: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(super) fn release(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job: &ReceiverJob,
    instance: &str,
    session_id: &str,
    now: i64,
) -> Result<bool> {
    let Some(registration) = registration(
        transaction,
        workspace_id,
        job.id(),
        job.token(),
        instance,
        session_id,
    )?
    else {
        return Ok(false);
    };
    if transaction.execute(
        "UPDATE brain_sessions SET locked_pid = NULL, last_active_at = ?7
         WHERE workspace_id = ?1 AND brain_instance_id = ?2
           AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
           AND agent_session_id = ?6 AND locked_pid = ?8
           AND source = 'fresh'",
        rusqlite::params![
            workspace_id,
            instance,
            &registration.agent_kind,
            &registration.actor_id,
            &registration.channel,
            session_id,
            now,
            registration.locked_pid,
        ],
    )? != 1
    {
        return Ok(false);
    }
    Ok(transaction.execute(
        "DELETE FROM receiver_session_registrations
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND brain_instance_id = ?3 AND agent_kind = ?4
           AND actor_id = ?5 AND channel = ?6
           AND registered_session_id = ?7
           AND actual_session_id IS ?8",
        rusqlite::params![
            workspace_id,
            job.conversation_id().to_string(),
            instance,
            &registration.agent_kind,
            &registration.actor_id,
            &registration.channel,
            &registration.registered_session_id,
            &registration.actual_session_id,
        ],
    )? == 1)
}
