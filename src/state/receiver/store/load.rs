use anyhow::{Context, Result};
use rusqlite::OptionalExtension as _;

use super::super::{
    ReceiverObservationMetadata, ReceiverRecoveryMetadata, ReceiverRetryMetadata,
    ReceiverStoredMetadata,
};
use crate::state::{
    ReceiverAttemptKind, ReceiverConversation, ReceiverConversationId,
    ReceiverConversationIdentity, ReceiverJob, ReceiverJobId, ReceiverJobState,
    ReceiverSessionBinding,
};

pub(super) fn load_receiver_job(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    job_id: ReceiverJobId,
) -> Result<Option<ReceiverJob>> {
    let stored = connection
        .query_row(
            "SELECT job_token, conversation_id, inbound_json, state, retry_count,
                    retry_at_unix_ms, retry_from_state, last_error,
                    launched_at_unix_ms, accepted_at_unix_ms, progressing_at_unix_ms,
                    completed_at_unix_ms, observation_instance, observation_session_id,
                    observation_revision, attempt_accepted_at_unix_ms,
                    attempt_progressing_at_unix_ms, latest_progress_at_unix_ms,
                    launch_expires_at_unix_ms, acceptance_expires_at_unix_ms,
                    progress_expires_at_unix_ms, recovery_expires_at_unix_ms,
                    absolute_work_expires_at_unix_ms, recovery_count, attempt_kind,
                    pending_unavailable_notice
             FROM receiver_jobs WHERE workspace_id = ?1 AND job_id = ?2",
            rusqlite::params![workspace_id, job_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, Option<i64>>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                    row.get::<_, Option<i64>>(17)?,
                    row.get::<_, Option<i64>>(18)?,
                    row.get::<_, Option<i64>>(19)?,
                    row.get::<_, Option<i64>>(20)?,
                    row.get::<_, Option<i64>>(21)?,
                    row.get::<_, Option<i64>>(22)?,
                    row.get::<_, i64>(23)?,
                    row.get::<_, String>(24)?,
                    row.get::<_, bool>(25)?,
                ))
            },
        )
        .optional()?;
    let Some((
        token,
        conversation,
        inbound_json,
        state,
        retry_count,
        retry_at,
        retry_from,
        last_error,
        launched,
        accepted,
        progressing,
        completed,
        instance,
        session_id,
        revision,
        attempt_accepted,
        attempt_progressing,
        latest_progress,
        launch_expires,
        acceptance_expires,
        progress_expires,
        recovery_expires,
        absolute_work_expires,
        recovery_count,
        attempt_kind,
        pending_unavailable_notice,
    )) = stored
    else {
        return Ok(None);
    };
    let inbound = serde_json::from_str(&inbound_json).context("parse durable receiver job")?;
    let state = ReceiverJobState::parse(&state)
        .ok_or_else(|| anyhow::anyhow!("unknown durable receiver job state {state:?}"))?;
    Ok(Some(ReceiverJob::from_stored(
        job_id,
        crate::state::ReceiverJobToken::parse(&token)?,
        ReceiverConversationId::parse(&conversation)?,
        inbound,
        ReceiverStoredMetadata {
            state,
            retry: ReceiverRetryMetadata {
                count: u32::try_from(retry_count).context("receiver retry count is outside u32")?,
                at_unix_ms: retry_at
                    .map(|value| from_i64(value, "receiver retry timestamp"))
                    .transpose()?,
                from_state: retry_from
                    .map(|value| {
                        ReceiverJobState::parse(&value).ok_or_else(|| {
                            anyhow::anyhow!("unknown receiver retry origin {value:?}")
                        })
                    })
                    .transpose()?,
                last_error,
            },
            observation: ReceiverObservationMetadata {
                launched_at_unix_ms: launched
                    .map(|value| from_i64(value, "receiver launched timestamp"))
                    .transpose()?,
                accepted_at_unix_ms: accepted
                    .map(|value| from_i64(value, "receiver accepted timestamp"))
                    .transpose()?,
                progressing_at_unix_ms: progressing
                    .map(|value| from_i64(value, "receiver progressing timestamp"))
                    .transpose()?,
                completed_at_unix_ms: completed
                    .map(|value| from_i64(value, "receiver completed timestamp"))
                    .transpose()?,
                instance,
                session_id,
                revision: from_i64(revision, "receiver observation revision")?,
                attempt_accepted_at_unix_ms: attempt_accepted
                    .map(|value| from_i64(value, "receiver attempt accepted timestamp"))
                    .transpose()?,
                attempt_progressing_at_unix_ms: attempt_progressing
                    .map(|value| from_i64(value, "receiver attempt progressing timestamp"))
                    .transpose()?,
            },
            recovery: ReceiverRecoveryMetadata {
                latest_progress_at_unix_ms: latest_progress
                    .map(|value| from_i64(value, "receiver latest progress timestamp"))
                    .transpose()?,
                launch_expires_at_unix_ms: launch_expires
                    .map(|value| from_i64(value, "receiver launch expiry"))
                    .transpose()?,
                acceptance_expires_at_unix_ms: acceptance_expires
                    .map(|value| from_i64(value, "receiver acceptance expiry"))
                    .transpose()?,
                progress_expires_at_unix_ms: progress_expires
                    .map(|value| from_i64(value, "receiver progress expiry"))
                    .transpose()?,
                recovery_expires_at_unix_ms: recovery_expires
                    .map(|value| from_i64(value, "receiver recovery expiry"))
                    .transpose()?,
                absolute_work_expires_at_unix_ms: absolute_work_expires
                    .map(|value| from_i64(value, "receiver absolute-work expiry"))
                    .transpose()?,
                recovery_count: u32::try_from(recovery_count)
                    .context("receiver recovery count is outside u32")?,
                attempt_kind: ReceiverAttemptKind::parse(&attempt_kind).ok_or_else(|| {
                    anyhow::anyhow!("unknown receiver attempt kind {attempt_kind:?}")
                })?,
                pending_unavailable_notice,
            },
        },
    )))
}

pub(super) fn load_receiver_conversation(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    conversation_id: ReceiverConversationId,
) -> Result<Option<ReceiverConversation>> {
    let stored = connection
        .query_row(
            "SELECT workspace_id, user_id, channel, conversation_key,
                    transcript_markdown, agent_kind, agent_session_id
             FROM receiver_conversations
             WHERE workspace_id = ?1 AND conversation_id = ?2",
            rusqlite::params![workspace_id, conversation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((workspace, user, channel, key, transcript, agent, native_session)) = stored else {
        return Ok(None);
    };
    let identity = ReceiverConversationIdentity::from_stored_parts(
        crate::workspace::WorkspaceId::parse(&workspace)?,
        crate::users::UserId::parse(&user)?,
        parse_channel(&channel)?,
        key,
    );
    let binding = match (agent, native_session) {
        (Some(agent), Some(native_session)) => Some(ReceiverSessionBinding::new(
            parse_agent_kind(&agent)?,
            native_session,
        )?),
        (None, None) => None,
        _ => return Err(anyhow::anyhow!("incomplete receiver session binding")),
    };
    Ok(Some(ReceiverConversation::from_stored(
        conversation_id,
        identity,
        transcript,
        binding,
    )))
}

fn parse_channel(value: &str) -> Result<crate::server::receiver::Channel> {
    match value {
        "sms" => Ok(crate::server::receiver::Channel::Sms),
        "email" => Ok(crate::server::receiver::Channel::Email),
        _ => Err(anyhow::anyhow!("unknown receiver channel {value:?}")),
    }
}

fn parse_agent_kind(value: &str) -> Result<crate::agent::AgentKind> {
    match value {
        "claude" => Ok(crate::agent::AgentKind::Claude),
        "codex" => Ok(crate::agent::AgentKind::Codex),
        "opencode" => Ok(crate::agent::AgentKind::OpenCode),
        _ => Err(anyhow::anyhow!("unknown receiver frontend {value:?}")),
    }
}

fn from_i64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{name} cannot be negative"))
}
