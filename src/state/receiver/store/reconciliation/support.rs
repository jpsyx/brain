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

pub(super) fn bind_exact_recovery_session(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    job: &ReceiverJob,
    now: i64,
) -> Result<bool> {
    let (Some(instance), Some(session_id)) =
        (job.observation_instance(), job.observation_session_id())
    else {
        return Ok(false);
    };
    let agent_kind = transaction
        .query_row(
            "SELECT registration.agent_kind
             FROM receiver_session_registrations AS registration
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
             WHERE registration.workspace_id = ?1
               AND registration.conversation_id = ?2
               AND registration.brain_instance_id = ?3
               AND session.agent_session_id = ?4",
            rusqlite::params![
                workspace_id,
                job.conversation_id().to_string(),
                instance,
                session_id,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(agent_kind) = agent_kind else {
        return Ok(false);
    };
    Ok(transaction.execute(
        "UPDATE receiver_conversations
         SET agent_kind = ?3, agent_session_id = ?4, updated_at_unix_ms = ?5
         WHERE workspace_id = ?1 AND conversation_id = ?2",
        rusqlite::params![
            workspace_id,
            job.conversation_id().to_string(),
            agent_kind,
            session_id,
            now,
        ],
    )? == 1)
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
                          'processing', 'answer-ready', 'delivering')
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
