use anyhow::Result;
use rusqlite::OptionalExtension as _;

use crate::state::{ReceiverJob, ReceiverJobId, ReceiverJobState};

pub(super) struct ReconciliationCandidate {
    pub(super) job_id: ReceiverJobId,
    pub(super) state: ReceiverJobState,
    pub(super) owner: Option<String>,
    pub(super) claim_expires_at_unix_ms: Option<i64>,
    pub(super) updated_at_unix_ms: i64,
    pub(super) exact_snapshot: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecoverySessionAttribution {
    Bound,
    FreshConflict,
    Absent,
}

pub(super) const EXACT_SNAPSHOT_SQL: &str = "json_array(
    conversation_id, job_token, state, claim_owner, claim_expires_at_unix_ms,
    retry_count, retry_at_unix_ms, retry_from_state, last_error,
    launched_at_unix_ms, accepted_at_unix_ms, progressing_at_unix_ms,
    completed_at_unix_ms, observation_instance, observation_session_id,
    observation_revision, attempt_accepted_at_unix_ms,
    attempt_progressing_at_unix_ms, latest_progress_at_unix_ms,
    launch_expires_at_unix_ms, acceptance_expires_at_unix_ms,
    progress_expires_at_unix_ms, recovery_expires_at_unix_ms,
    absolute_work_expires_at_unix_ms, recovery_count, attempt_kind,
    pending_unavailable_notice, recovery_cleanup_instance,
    recovery_cleanup_session_id, updated_at_unix_ms
)";

pub(super) fn attribute_exact_recovery_session(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job: &ReceiverJob,
    now: i64,
) -> Result<RecoverySessionAttribution> {
    let (Some(instance), Some(session_id)) =
        (job.observation_instance(), job.observation_session_id())
    else {
        return Ok(RecoverySessionAttribution::Absent);
    };
    let actor_id = job.inbound().actor.user_id().as_str();
    let channel = super::super::channel_str(job.inbound().channel);
    let registration = transaction
        .query_row(
            "SELECT registration.agent_kind, registration.registered_session_id
             FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.agent_session_id = ?4
              AND session.locked_pid IS NOT NULL
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
               AND registration.actor_id = ?5
               AND registration.channel = ?6
               AND (registration.actual_session_id IS NULL
                    OR registration.actual_session_id = ?4)
               AND (conversation.agent_kind IS NULL
                    OR conversation.agent_kind = registration.agent_kind)
               AND (conversation.agent_session_id IS NULL
                    OR conversation.agent_session_id = ?4)
               AND job.job_id = ?7 AND job.job_token = ?8
               AND job.observation_instance = ?3
               AND job.observation_session_id = ?4",
            rusqlite::params![
                workspace_id,
                job.conversation_id().to_string(),
                instance,
                session_id,
                actor_id,
                channel,
                job.id().to_string(),
                job.token().to_string(),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((agent_kind, registered_session_id)) = registration else {
        return exact_fresh_conflict(transaction, workspace_id, job);
    };
    if transaction.execute(
        "UPDATE receiver_session_registrations AS registration
         SET actual_session_id = ?4
         WHERE registration.workspace_id = ?1
           AND registration.conversation_id = ?2
           AND registration.brain_instance_id = ?3
           AND (registration.actual_session_id IS NULL
                OR registration.actual_session_id = ?4)
           AND registration.registered_session_id = ?5
           AND registration.agent_kind = ?6
           AND registration.actor_id = ?7
           AND registration.channel = ?8
           AND EXISTS (
             SELECT 1 FROM brain_sessions AS session
             WHERE session.workspace_id = ?1
               AND session.brain_instance_id = ?3
               AND session.agent_kind = ?6
               AND session.actor_id = ?7
               AND session.channel = ?8
               AND session.agent_session_id = ?4
               AND session.locked_pid IS NOT NULL
           )
           AND EXISTS (
             SELECT 1 FROM receiver_conversations AS conversation
             WHERE conversation.workspace_id = ?1
               AND conversation.conversation_id = ?2
               AND conversation.user_id = ?7
               AND conversation.channel = ?8
               AND (conversation.agent_kind IS NULL
                    OR conversation.agent_kind = ?6)
               AND (conversation.agent_session_id IS NULL
                    OR conversation.agent_session_id = ?4)
           )
           AND EXISTS (
             SELECT 1 FROM receiver_jobs AS job
             WHERE job.workspace_id = ?1 AND job.job_id = ?9
               AND job.job_token = ?10 AND job.conversation_id = ?2
               AND job.channel = ?8 AND job.observation_instance = ?3
               AND job.observation_session_id = ?4
           )",
        rusqlite::params![
            workspace_id,
            job.conversation_id().to_string(),
            instance,
            session_id,
            registered_session_id,
            agent_kind,
            actor_id,
            channel,
            job.id().to_string(),
            job.token().to_string(),
        ],
    )? != 1
    {
        return Ok(RecoverySessionAttribution::Absent);
    }
    let bound = transaction.execute(
        "UPDATE receiver_conversations
         SET agent_kind = ?3, agent_session_id = ?4, updated_at_unix_ms = ?5
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND user_id = ?6 AND channel = ?7
           AND ((agent_kind IS NULL AND agent_session_id IS NULL)
                OR (agent_kind = ?3 AND agent_session_id = ?4))
           AND EXISTS (
             SELECT 1 FROM receiver_session_registrations AS registration
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
             WHERE registration.workspace_id = ?1
               AND registration.conversation_id = ?2
               AND registration.agent_kind = ?3
               AND registration.actor_id = ?6
               AND registration.channel = ?7
               AND registration.brain_instance_id = ?8
               AND registration.registered_session_id = ?9
               AND registration.actual_session_id = ?4
               AND session.agent_session_id = ?4
               AND session.locked_pid IS NOT NULL
           )
           AND EXISTS (
             SELECT 1 FROM receiver_jobs AS job
             WHERE job.workspace_id = ?1 AND job.job_id = ?10
               AND job.job_token = ?11 AND job.conversation_id = ?2
               AND job.channel = ?7 AND job.observation_instance = ?8
               AND job.observation_session_id = ?4
           )",
        rusqlite::params![
            workspace_id,
            job.conversation_id().to_string(),
            agent_kind,
            session_id,
            now,
            actor_id,
            channel,
            instance,
            registered_session_id,
            job.id().to_string(),
            job.token().to_string(),
        ],
    )? == 1;
    anyhow::ensure!(
        bound,
        "exact receiver recovery binding changed within its immediate transaction"
    );
    Ok(RecoverySessionAttribution::Bound)
}

fn exact_fresh_conflict(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job: &ReceiverJob,
) -> Result<RecoverySessionAttribution> {
    let (Some(instance), Some(session_id)) =
        (job.observation_instance(), job.observation_session_id())
    else {
        return Ok(RecoverySessionAttribution::Absent);
    };
    let exact = transaction.query_row(
        "SELECT EXISTS (
           SELECT 1 FROM receiver_session_registrations AS registration
           JOIN brain_sessions AS session
             ON session.workspace_id = registration.workspace_id
            AND session.brain_instance_id = registration.brain_instance_id
            AND session.agent_kind = registration.agent_kind
            AND session.actor_id = registration.actor_id
            AND session.channel = registration.channel
            AND session.agent_session_id = ?4
            AND session.locked_pid IS NOT NULL
            AND session.source = 'fresh'
           JOIN receiver_conversations AS conversation
             ON conversation.workspace_id = registration.workspace_id
            AND conversation.conversation_id = registration.conversation_id
            AND conversation.user_id = registration.actor_id
            AND conversation.channel = registration.channel
           JOIN receiver_jobs AS exact_job
             ON exact_job.workspace_id = conversation.workspace_id
            AND exact_job.conversation_id = conversation.conversation_id
            AND exact_job.channel = conversation.channel
           WHERE registration.workspace_id = ?1
             AND registration.conversation_id = ?2
             AND registration.brain_instance_id = ?3
             AND registration.actor_id = ?5
             AND registration.channel = ?6
             AND registration.registered_session_id != ?4
             AND conversation.agent_kind = registration.agent_kind
             AND conversation.agent_session_id IS NOT NULL
             AND conversation.agent_session_id != ?4
             AND conversation.agent_session_id != registration.registered_session_id
             AND (registration.actual_session_id IS NULL
                  OR registration.actual_session_id = conversation.agent_session_id)
             AND exact_job.job_id = ?7 AND exact_job.job_token = ?8
             AND exact_job.attempt_kind = 'ordinary'
             AND exact_job.observation_instance = ?3
             AND exact_job.observation_session_id = ?4
         )",
        rusqlite::params![
            workspace_id,
            job.conversation_id().to_string(),
            instance,
            session_id,
            job.inbound().actor.user_id().as_str(),
            super::super::channel_str(job.inbound().channel),
            job.id().to_string(),
            job.token().to_string(),
        ],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(if exact {
        RecoverySessionAttribution::FreshConflict
    } else {
        RecoverySessionAttribution::Absent
    })
}

pub(super) fn oldest_blocking_candidate(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
) -> Result<Option<ReconciliationCandidate>> {
    let sql = format!(
        "SELECT job_id, state, claim_owner, claim_expires_at_unix_ms,
                updated_at_unix_ms, {EXACT_SNAPSHOT_SQL}
         FROM receiver_jobs
         WHERE workspace_id = ?1
           AND (state IN ('claimed', 'launching', 'launched', 'accepted',
                          'processing')
                OR (state IN ('answer-ready', 'delivering') AND NOT EXISTS (
                     SELECT 1 FROM receiver_deliveries AS delivery
                     WHERE delivery.job_id = receiver_jobs.job_id
                       AND delivery.job_token = receiver_jobs.job_token
                       AND delivery.response_kind = 'final-answer'
                   ))
                OR (state = 'retrying' AND attempt_kind = 'recovery'))
         ORDER BY received_at_unix_ms, job_id
         LIMIT 1"
    );
    let stored = transaction
        .query_row(&sql, [workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .optional()?;
    let Some((job_id, state, owner, claim_expires_at_unix_ms, updated_at_unix_ms, exact_snapshot)) =
        stored
    else {
        return Ok(None);
    };
    let state = ReceiverJobState::parse(&state)
        .ok_or_else(|| anyhow::anyhow!("unknown durable receiver job state {state:?}"))?;
    Ok(Some(ReconciliationCandidate {
        job_id: ReceiverJobId::parse(&job_id)?,
        state,
        owner,
        claim_expires_at_unix_ms,
        updated_at_unix_ms,
        exact_snapshot,
    }))
}

pub(super) fn candidate_for_job(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job_id: ReceiverJobId,
) -> Result<Option<ReconciliationCandidate>> {
    let sql = format!(
        "SELECT state, claim_owner, claim_expires_at_unix_ms,
                updated_at_unix_ms, {EXACT_SNAPSHOT_SQL}
         FROM receiver_jobs WHERE workspace_id = ?1 AND job_id = ?2"
    );
    let stored = transaction
        .query_row(
            &sql,
            rusqlite::params![workspace_id, job_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((state, owner, claim_expires_at_unix_ms, updated_at_unix_ms, exact_snapshot)) = stored
    else {
        return Ok(None);
    };
    let state = ReceiverJobState::parse(&state)
        .ok_or_else(|| anyhow::anyhow!("unknown durable receiver job state {state:?}"))?;
    Ok(Some(ReconciliationCandidate {
        job_id,
        state,
        owner,
        claim_expires_at_unix_ms,
        updated_at_unix_ms,
        exact_snapshot,
    }))
}

pub(super) fn release_registration(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    conversation_id: crate::state::ReceiverConversationId,
    instance: Option<&str>,
    now: i64,
) -> Result<()> {
    let Some(instance) = instance else {
        return Ok(());
    };
    transaction.execute(
        "UPDATE brain_sessions SET locked_pid = NULL, last_active_at = ?4
         WHERE workspace_id = ?1 AND brain_instance_id = ?2
           AND EXISTS (
             SELECT 1 FROM receiver_session_registrations AS registration
             WHERE registration.workspace_id = ?1
               AND registration.conversation_id = ?3
               AND registration.brain_instance_id = ?2
               AND registration.agent_kind = brain_sessions.agent_kind
               AND registration.actor_id = brain_sessions.actor_id
               AND registration.channel = brain_sessions.channel
           )",
        rusqlite::params![workspace_id, instance, conversation_id.to_string(), now],
    )?;
    transaction.execute(
        "DELETE FROM receiver_session_registrations
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND brain_instance_id = ?3",
        rusqlite::params![workspace_id, conversation_id.to_string(), instance],
    )?;
    Ok(())
}
