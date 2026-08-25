use anyhow::{Context, Result};
use rusqlite::OptionalExtension as _;

use super::{
    ReceiverAcceptance, ReceiverConversation, ReceiverConversationId, ReceiverConversationIdentity,
    ReceiverJob, ReceiverJobId, ReceiverJobToken, ReceiverSessionBinding,
};
use crate::state::Db;

mod claim;
mod completion;
mod control;
mod load;
mod observation;
mod session;

use load::{load_receiver_conversation, load_receiver_job};

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
        let token = ReceiverJobToken::new();
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
               (job_id, job_token, workspace_id, conversation_id, channel, provider_id,
                inbound_json, state, received_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?8)",
            rusqlite::params![
                job_id.to_string(),
                token.to_string(),
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
        load_receiver_job(&self.conn, &self.workspace_id, job_id)
    }

    /// Load one logical receiver conversation and its portable transcript.
    pub fn receiver_conversation(
        &self,
        conversation_id: ReceiverConversationId,
    ) -> Result<Option<ReceiverConversation>> {
        load_receiver_conversation(&self.conn, &self.workspace_id, conversation_id)
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

pub(super) fn to_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} is outside SQLite integer range"))
}
