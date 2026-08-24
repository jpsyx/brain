use anyhow::{Context, Result};
use rusqlite::OptionalExtension as _;

use super::{
    ReceiverAcceptance, ReceiverConversation, ReceiverConversationId, ReceiverConversationIdentity,
    ReceiverJob, ReceiverJobId, ReceiverJobState, ReceiverSessionBinding,
};
use crate::state::Db;

mod claim;

const QUEUED_JOB_LIMIT: i64 = 64;

impl Db {
    /// Persist one authenticated inbound job before acknowledging its provider.
    pub fn accept_receiver_job(
        &self,
        inbound: &crate::server::receiver::InboundJob,
        identity: &ReceiverConversationIdentity,
    ) -> Result<ReceiverAcceptance> {
        self.validate_receiver_scope(inbound, identity)?;
        let channel = channel_str(inbound.channel);
        let job_id = ReceiverJobId::from(inbound.job_id);
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;

        if let Some((stored_job, conversation)) = inbound
            .provider_id
            .as_deref()
            .map(|provider_id| {
                transaction
                    .query_row(
                        "SELECT job_id, conversation_id FROM receiver_jobs
                         WHERE workspace_id = ?1 AND channel = ?2 AND provider_id = ?3",
                        rusqlite::params![self.workspace_id, channel, provider_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()
            })
            .transpose()?
            .flatten()
        {
            return Ok(ReceiverAcceptance::new(
                ReceiverJobId::parse(&stored_job)?,
                ReceiverConversationId::parse(&conversation)?,
                false,
            ));
        }

        if let Some(conversation) = transaction
            .query_row(
                "SELECT conversation_id FROM receiver_jobs
                 WHERE workspace_id = ?1 AND job_id = ?2",
                rusqlite::params![self.workspace_id, job_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(ReceiverAcceptance::new(
                job_id,
                ReceiverConversationId::parse(&conversation)?,
                false,
            ));
        }

        let queued_jobs = transaction.query_row(
            "SELECT COUNT(*) FROM receiver_jobs
             WHERE workspace_id = ?1 AND state = 'queued'",
            [self.workspace_id.as_str()],
            |row| row.get::<_, i64>(0),
        )?;
        anyhow::ensure!(
            queued_jobs < QUEUED_JOB_LIMIT,
            "receiver queued-job capacity of {QUEUED_JOB_LIMIT} is full"
        );

        let conversation_id = transaction
            .query_row(
                "SELECT conversation_id FROM receiver_conversations
                 WHERE workspace_id = ?1 AND user_id = ?2
                   AND channel = ?3 AND conversation_key = ?4",
                rusqlite::params![
                    self.workspace_id,
                    identity.user_id().as_str(),
                    channel,
                    identity.conversation_key(),
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map_or_else(
                || -> Result<ReceiverConversationId> {
                    let id = ReceiverConversationId::new();
                    transaction.execute(
                        "INSERT INTO receiver_conversations
                           (conversation_id, workspace_id, user_id, channel,
                            conversation_key, transcript_markdown, created_at_unix_ms,
                            updated_at_unix_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5, '', ?6, ?6)",
                        rusqlite::params![
                            id.to_string(),
                            self.workspace_id,
                            identity.user_id().as_str(),
                            channel,
                            identity.conversation_key(),
                            to_i64(inbound.received_at_unix_ms, "conversation timestamp",)?,
                        ],
                    )?;
                    Ok(id)
                },
                |value| ReceiverConversationId::parse(&value),
            )?;

        let received_at = to_i64(inbound.received_at_unix_ms, "receiver job timestamp")?;
        let inbound_json = serde_json::to_string(inbound).context("serialize receiver job")?;
        transaction.execute(
            "INSERT INTO receiver_jobs
               (job_id, workspace_id, conversation_id, channel, provider_id,
                inbound_json, state, received_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?7)",
            rusqlite::params![
                job_id.to_string(),
                self.workspace_id,
                conversation_id.to_string(),
                channel,
                inbound.provider_id,
                inbound_json,
                received_at,
            ],
        )?;
        transaction.commit()?;
        Ok(ReceiverAcceptance::new(job_id, conversation_id, true))
    }

    /// Load one durable receiver job without changing queue ownership.
    pub fn receiver_job(&self, job_id: ReceiverJobId) -> Result<Option<ReceiverJob>> {
        let stored = self
            .conn
            .query_row(
                "SELECT conversation_id, inbound_json, state, retry_count,
                        retry_at_unix_ms, last_error
                 FROM receiver_jobs WHERE workspace_id = ?1 AND job_id = ?2",
                rusqlite::params![self.workspace_id, job_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((conversation, inbound_json, state, retry_count, retry_at, last_error)) = stored
        else {
            return Ok(None);
        };
        let inbound = serde_json::from_str(&inbound_json).context("parse durable receiver job")?;
        let state = ReceiverJobState::parse(&state)
            .ok_or_else(|| anyhow::anyhow!("unknown durable receiver job state {state:?}"))?;
        Ok(Some(ReceiverJob::from_stored(
            job_id,
            ReceiverConversationId::parse(&conversation)?,
            inbound,
            state,
            u32::try_from(retry_count).context("receiver retry count is outside u32")?,
            retry_at
                .map(|value| from_i64(value, "receiver retry timestamp"))
                .transpose()?,
            last_error,
        )))
    }

    /// Load one logical receiver conversation and its portable transcript.
    pub fn receiver_conversation(
        &self,
        conversation_id: ReceiverConversationId,
    ) -> Result<Option<ReceiverConversation>> {
        let stored = self
            .conn
            .query_row(
                "SELECT workspace_id, user_id, channel, conversation_key,
                        transcript_markdown, agent_kind, agent_session_id
                 FROM receiver_conversations
                 WHERE workspace_id = ?1 AND conversation_id = ?2",
                rusqlite::params![self.workspace_id, conversation_id.to_string()],
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
        let Some((workspace, user, channel, key, transcript, agent, native_session)) = stored
        else {
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

    /// Atomically replace the portable transcript and current native binding.
    pub fn update_receiver_conversation(
        &self,
        conversation_id: ReceiverConversationId,
        transcript_markdown: &str,
        binding: Option<&ReceiverSessionBinding>,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let (agent_kind, native_session_id) = binding.map_or((None, None), |binding| {
            (
                Some(binding.frontend().as_str()),
                Some(binding.native_session_id()),
            )
        });
        Ok(self.conn.execute(
            "UPDATE receiver_conversations
             SET transcript_markdown = ?3, agent_kind = ?4, agent_session_id = ?5,
                 updated_at_unix_ms = ?6
             WHERE workspace_id = ?1 AND conversation_id = ?2",
            rusqlite::params![
                self.workspace_id,
                conversation_id.to_string(),
                transcript_markdown,
                agent_kind,
                native_session_id,
                to_i64(observed_at_unix_ms, "conversation update timestamp")?,
            ],
        )? == 1)
    }

    fn validate_receiver_scope(
        &self,
        inbound: &crate::server::receiver::InboundJob,
        identity: &ReceiverConversationIdentity,
    ) -> Result<()> {
        anyhow::ensure!(
            inbound.workspace_id.to_string() == self.workspace_id,
            "receiver job belongs to another workspace"
        );
        anyhow::ensure!(
            identity.workspace_id().to_string() == self.workspace_id,
            "receiver conversation belongs to another workspace"
        );
        anyhow::ensure!(
            inbound.actor.user_id() == identity.user_id(),
            "receiver job actor does not match conversation user"
        );
        anyhow::ensure!(
            inbound.channel == identity.channel(),
            "receiver job channel does not match conversation channel"
        );
        Ok(())
    }
}

pub(super) fn validated_owner(owner: &str) -> Result<&str> {
    let owner = owner.trim();
    anyhow::ensure!(!owner.is_empty(), "receiver claim owner cannot be blank");
    Ok(owner)
}

fn channel_str(channel: crate::server::receiver::Channel) -> &'static str {
    match channel {
        crate::server::receiver::Channel::Sms => "sms",
        crate::server::receiver::Channel::Email => "email",
    }
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

pub(super) fn to_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} is outside SQLite integer range"))
}

fn from_i64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{name} cannot be negative"))
}
