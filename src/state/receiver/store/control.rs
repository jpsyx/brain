use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::server::receiver::{ControlCommand, InboundJob, RestartPlan, parse_control_command};
use crate::state::{Db, ReceiverConversationId, ReceiverJobId};

struct ControlRow {
    conversation_id: ReceiverConversationId,
    received_at_unix_ms: i64,
    user_id: String,
    channel: String,
    conversation_key: String,
}

impl Db {
    /// Complete an exact claimed `/new` and roll later work onto a fresh conversation.
    pub fn complete_receiver_new_session(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        let observed = to_i64(observed_at_unix_ms, "receiver new-session time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let control = transaction
            .query_row(
                "SELECT job.conversation_id, job.received_at_unix_ms, job.inbound_json,
                        job.response_sender,
                        conversation.user_id, conversation.channel,
                        conversation.conversation_key
                 FROM receiver_jobs AS job
                 JOIN receiver_conversations AS conversation
                   ON conversation.conversation_id = job.conversation_id
                  AND conversation.workspace_id = job.workspace_id
                 WHERE job.workspace_id = ?1 AND job.job_id = ?2
                   AND job.state = 'claimed' AND job.claim_owner = ?3
                   AND job.claim_expires_at_unix_ms > ?4",
                rusqlite::params![self.workspace_id, job_id.to_string(), owner, observed],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            conversation_id,
            received_at_unix_ms,
            inbound_json,
            response_sender,
            user_id,
            channel,
            key,
        )) = control
        else {
            return Ok(false);
        };
        let inbound = super::decode_inbound(&inbound_json, response_sender)?;
        if parse_control_command(&inbound.prompt) != Some(ControlCommand::NewSession) {
            return Ok(false);
        }
        let control = ControlRow {
            conversation_id: ReceiverConversationId::parse(&conversation_id)?,
            received_at_unix_ms,
            user_id,
            channel,
            conversation_key: key,
        };
        if roll_conversation(&transaction, &self.workspace_id, &control, job_id, observed)?
            .is_none()
        {
            return Ok(false);
        }
        release_conversation_registrations(
            &transaction,
            &self.workspace_id,
            control.conversation_id,
            observed,
        )?;
        let completed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'done', claim_owner = NULL,
                 claim_expires_at_unix_ms = NULL, updated_at_unix_ms = ?4
             WHERE workspace_id = ?1 AND job_id = ?2 AND claim_owner = ?3
               AND claim_expires_at_unix_ms > ?4 AND state = 'claimed'",
            rusqlite::params![self.workspace_id, job_id.to_string(), owner, observed],
        )?;
        if completed != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Apply the oldest queued `/restart` as an atomic cut through prior waiting work.
    pub fn apply_next_receiver_restart(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<Option<RestartPlan<InboundJob>>> {
        let observed = to_i64(observed_at_unix_ms, "receiver restart time")?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let queued = {
            let mut statement = transaction.prepare(
                "SELECT job.job_id, job.conversation_id, job.received_at_unix_ms,
                        job.inbound_json, job.response_sender,
                        conversation.user_id, conversation.channel,
                        conversation.conversation_key
                 FROM receiver_jobs AS job
                 JOIN receiver_conversations AS conversation
                   ON conversation.conversation_id = job.conversation_id
                  AND conversation.workspace_id = job.workspace_id
                 WHERE job.workspace_id = ?1 AND job.state = 'queued'
                   AND job.claim_owner IS NULL
                 ORDER BY job.received_at_unix_ms, job.job_id",
            )?;
            statement
                .query_map([self.workspace_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let restart = queued.into_iter().find_map(
            |(
                job_id,
                conversation_id,
                received_at,
                inbound_json,
                response_sender,
                user,
                channel,
                key,
            )| {
                let inbound = super::decode_inbound(&inbound_json, response_sender).ok()?;
                (parse_control_command(&inbound.prompt) == Some(ControlCommand::Restart)).then_some(
                    (
                        job_id,
                        ControlRow {
                            conversation_id: ReceiverConversationId::parse(&conversation_id)
                                .ok()?,
                            received_at_unix_ms: received_at,
                            user_id: user,
                            channel,
                            conversation_key: key,
                        },
                        inbound,
                    ),
                )
            },
        );
        let Some((restart_job_id, control, command)) = restart else {
            return Ok(None);
        };
        let restart_job_id = ReceiverJobId::parse(&restart_job_id)?;
        let dropped = load_restart_backlog(
            &transaction,
            &self.workspace_id,
            control.received_at_unix_ms,
            restart_job_id,
        )?;
        let dropped_count = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'failed', retry_at_unix_ms = NULL,
                 retry_from_state = NULL, last_error = 'dropped-by-restart',
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 updated_at_unix_ms = ?2
             WHERE workspace_id = ?1 AND claim_owner IS NULL
               AND (
                 received_at_unix_ms < ?3
                 OR (received_at_unix_ms = ?3 AND job_id < ?4)
               )
               AND (
                 state = 'queued'
                 OR (
                   state = 'retrying'
                   AND retry_from_state IN ('claimed', 'launching')
                 )
               )",
            rusqlite::params![
                self.workspace_id,
                observed,
                control.received_at_unix_ms,
                restart_job_id.to_string(),
            ],
        )?;
        anyhow::ensure!(
            dropped_count == dropped.len(),
            "receiver restart backlog changed during its transaction"
        );
        if roll_conversation(
            &transaction,
            &self.workspace_id,
            &control,
            restart_job_id,
            observed,
        )?
        .is_none()
        {
            return Ok(None);
        }
        release_conversation_registrations(
            &transaction,
            &self.workspace_id,
            control.conversation_id,
            observed,
        )?;
        let completed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'done', updated_at_unix_ms = ?3
             WHERE workspace_id = ?1 AND job_id = ?2
               AND state = 'queued' AND claim_owner IS NULL",
            rusqlite::params![self.workspace_id, restart_job_id.to_string(), observed],
        )?;
        if completed != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(RestartPlan { command, dropped }))
    }
}

fn load_restart_backlog(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    restart_received_at_unix_ms: i64,
    restart_job_id: ReceiverJobId,
) -> Result<Vec<InboundJob>> {
    let mut statement = transaction.prepare(
        "SELECT inbound_json, response_sender FROM receiver_jobs
         WHERE workspace_id = ?1 AND claim_owner IS NULL
           AND (
             received_at_unix_ms < ?2
             OR (received_at_unix_ms = ?2 AND job_id < ?3)
           )
           AND (
             state = 'queued'
             OR (
               state = 'retrying'
               AND retry_from_state IN ('claimed', 'launching')
             )
           )
         ORDER BY received_at_unix_ms, job_id",
    )?;
    statement
        .query_map(
            rusqlite::params![
                workspace_id,
                restart_received_at_unix_ms,
                restart_job_id.to_string(),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )?
        .map(|row| {
            let (inbound_json, response_sender) = row?;
            super::decode_inbound(&inbound_json, response_sender)
        })
        .collect()
}

fn roll_conversation(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    control: &ControlRow,
    control_job_id: ReceiverJobId,
    observed_at_unix_ms: i64,
) -> Result<Option<ReceiverConversationId>> {
    let fresh_conversation = ReceiverConversationId::new();
    let retired_key = format!(
        "retired:{}:{}",
        control.conversation_id, control.conversation_key
    );
    let retired = transaction.execute(
        "UPDATE receiver_conversations
         SET conversation_key = ?3, updated_at_unix_ms = ?4
         WHERE workspace_id = ?1 AND conversation_id = ?2",
        rusqlite::params![
            workspace_id,
            control.conversation_id.to_string(),
            retired_key,
            observed_at_unix_ms,
        ],
    )?;
    if retired != 1 {
        return Ok(None);
    }
    transaction.execute(
        "INSERT INTO receiver_conversations
           (conversation_id, workspace_id, user_id, channel,
            conversation_key, transcript_markdown, created_at_unix_ms,
            updated_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, '', ?6, ?6)",
        rusqlite::params![
            fresh_conversation.to_string(),
            workspace_id,
            control.user_id,
            control.channel,
            control.conversation_key,
            observed_at_unix_ms,
        ],
    )?;
    transaction.execute(
        "UPDATE receiver_jobs SET conversation_id = ?3, updated_at_unix_ms = ?4
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND (
             received_at_unix_ms > ?5
             OR (received_at_unix_ms = ?5 AND job_id > ?6)
           )
           AND claim_owner IS NULL
           AND (
             state = 'queued'
             OR (
               state = 'retrying'
               AND retry_from_state IN ('claimed', 'launching')
             )
           )",
        rusqlite::params![
            workspace_id,
            control.conversation_id.to_string(),
            fresh_conversation.to_string(),
            observed_at_unix_ms,
            control.received_at_unix_ms,
            control_job_id.to_string(),
        ],
    )?;
    Ok(Some(fresh_conversation))
}

fn release_conversation_registrations(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    conversation_id: ReceiverConversationId,
    observed_at_unix_ms: i64,
) -> Result<()> {
    transaction.execute(
        "UPDATE brain_sessions
         SET locked_pid = NULL, last_active_at = ?3
         WHERE workspace_id = ?1 AND locked_pid IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM receiver_jobs AS active_job
             WHERE active_job.workspace_id = ?1
               AND active_job.conversation_id = ?2
               AND active_job.claim_owner IS NOT NULL
               AND active_job.claim_expires_at_unix_ms > ?3
               AND active_job.state NOT IN ('claimed', 'failed', 'done')
           )
           AND EXISTS (
             SELECT 1 FROM receiver_session_registrations AS registration
             WHERE registration.workspace_id = ?1
               AND registration.conversation_id = ?2
               AND registration.brain_instance_id = brain_sessions.brain_instance_id
               AND registration.agent_kind = brain_sessions.agent_kind
               AND registration.actor_id = brain_sessions.actor_id
               AND registration.channel = brain_sessions.channel
           )",
        rusqlite::params![
            workspace_id,
            conversation_id.to_string(),
            observed_at_unix_ms
        ],
    )?;
    transaction.execute(
        "DELETE FROM receiver_session_registrations
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND NOT EXISTS (
             SELECT 1 FROM receiver_jobs AS active_job
             WHERE active_job.workspace_id = ?1
               AND active_job.conversation_id = ?2
               AND active_job.claim_owner IS NOT NULL
               AND active_job.claim_expires_at_unix_ms > ?3
               AND active_job.state NOT IN ('claimed', 'failed', 'done')
           )",
        rusqlite::params![
            workspace_id,
            conversation_id.to_string(),
            observed_at_unix_ms
        ],
    )?;
    Ok(())
}
