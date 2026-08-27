use anyhow::Result;
use rusqlite::OptionalExtension as _;

use crate::state::ReceiverJob;

pub(super) struct ExactRecoveryRegistration {
    pub(super) agent_kind: String,
    pub(super) session_id: String,
    pub(super) locked_pid: i32,
}

pub(super) fn exact_recovery_registration(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job: &ReceiverJob,
    instance: &str,
) -> Result<Option<ExactRecoveryRegistration>> {
    let stored = transaction
        .query_row(
            "SELECT registration.agent_kind, registration.registered_session_id,
                    session.locked_pid
             FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.agent_session_id = registration.registered_session_id
              AND session.locked_pid IS NOT NULL
              AND session.source IS NOT NULL
              AND TRIM(session.source) != ''
             JOIN receiver_conversations AS conversation
               ON conversation.workspace_id = registration.workspace_id
              AND conversation.conversation_id = registration.conversation_id
              AND conversation.user_id = registration.actor_id
              AND conversation.channel = registration.channel
              AND conversation.agent_kind = registration.agent_kind
              AND conversation.agent_session_id = registration.registered_session_id
             JOIN receiver_jobs AS exact_job
               ON exact_job.workspace_id = conversation.workspace_id
              AND exact_job.conversation_id = conversation.conversation_id
              AND exact_job.channel = conversation.channel
             WHERE registration.workspace_id = ?1
               AND registration.conversation_id = ?2
               AND registration.brain_instance_id = ?3
               AND registration.actor_id = ?4
               AND registration.channel = ?5
               AND (registration.actual_session_id IS NULL
                    OR registration.actual_session_id = registration.registered_session_id)
               AND exact_job.job_id = ?6 AND exact_job.job_token = ?7
               AND exact_job.attempt_kind = 'recovery'",
            rusqlite::params![
                workspace_id,
                job.conversation_id().to_string(),
                instance,
                job.inbound().actor.user_id().as_str(),
                super::super::channel_str(job.inbound().channel),
                job.id().to_string(),
                job.token().to_string(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((agent_kind, session_id, locked_pid)) = stored else {
        return Ok(None);
    };
    let Ok(locked_pid) = i32::try_from(locked_pid) else {
        return Ok(None);
    };
    if locked_pid <= 0 {
        return Ok(None);
    }
    Ok(Some(ExactRecoveryRegistration {
        agent_kind,
        session_id,
        locked_pid,
    }))
}
