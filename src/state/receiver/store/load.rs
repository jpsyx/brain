use anyhow::{Context, Result};
use rusqlite::OptionalExtension as _;

use super::super::{ReceiverObservationMetadata, ReceiverRetryMetadata};
use crate::state::{
    ReceiverConversation, ReceiverConversationId, ReceiverConversationIdentity, ReceiverJob,
    ReceiverJobId, ReceiverJobState, ReceiverSessionBinding,
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
                    observation_revision
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
        state,
        ReceiverRetryMetadata {
            count: u32::try_from(retry_count).context("receiver retry count is outside u32")?,
            at_unix_ms: retry_at
                .map(|value| from_i64(value, "receiver retry timestamp"))
                .transpose()?,
            from_state: retry_from
                .map(|value| {
                    ReceiverJobState::parse(&value)
                        .ok_or_else(|| anyhow::anyhow!("unknown receiver retry origin {value:?}"))
                })
                .transpose()?,
            last_error,
        },
        ReceiverObservationMetadata {
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
