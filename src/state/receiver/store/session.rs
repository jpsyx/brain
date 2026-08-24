use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::agent::{AgentSession, SessionScope};
use crate::state::{Db, ReceiverConversationId, ReceiverSessionAttribution};

impl Db {
    /// Register a fresh session and its exact logical receiver scope atomically.
    pub fn register_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<ReceiverSessionAttribution> {
        let instance = validated_owner(instance)?.to_owned();
        self.validate_receiver_session_scope(conversation_id, scope)?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let now = self.now();
        transaction.execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES (?1, ?2, ?3, ?4, 'fresh', ?5, ?6, ?7, ?8, ?8)",
            rusqlite::params![
                scope.agent_kind().as_str(),
                session.as_str(),
                instance,
                pid,
                self.workspace_id,
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                now,
            ],
        )?;
        insert_registration(
            &transaction,
            &self.workspace_id,
            conversation_id,
            session,
            &instance,
            scope,
        )?;
        transaction.commit()?;
        Ok(ReceiverSessionAttribution::new(
            conversation_id,
            instance,
            session.clone(),
            scope.clone(),
        ))
    }

    /// Claim one exact bound native session and retain its logical receiver scope.
    pub fn claim_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<Option<ReceiverSessionAttribution>> {
        let instance = validated_owner(instance)?.to_owned();
        self.validate_receiver_session_scope(conversation_id, scope)?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let changed = transaction.execute(
            "UPDATE brain_sessions
             SET locked_pid = ?2, brain_instance_id = ?3, last_active_at = ?4
             WHERE agent_session_id = ?1 AND locked_pid IS NULL
               AND agent_kind = ?5 AND workspace_id = ?6
               AND actor_id = ?7 AND channel = ?8
               AND EXISTS (
                 SELECT 1 FROM receiver_conversations
                 WHERE workspace_id = ?6 AND conversation_id = ?9
                   AND user_id = ?7 AND channel = ?8
                   AND agent_kind = ?5 AND agent_session_id = ?1
               )",
            rusqlite::params![
                session.as_str(),
                pid,
                instance,
                self.now(),
                scope.agent_kind().as_str(),
                self.workspace_id,
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                conversation_id.to_string(),
            ],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        insert_registration(
            &transaction,
            &self.workspace_id,
            conversation_id,
            session,
            &instance,
            scope,
        )?;
        transaction.commit()?;
        Ok(Some(ReceiverSessionAttribution::new(
            conversation_id,
            instance,
            session.clone(),
            scope.clone(),
        )))
    }

    /// Release and forget only one exact receiver session registration.
    pub fn release_receiver_session(
        &self,
        registration: &ReceiverSessionAttribution,
    ) -> Result<()> {
        self.validate_receiver_session_scope(registration.conversation_id(), registration.scope())?;
        let scope = registration.scope();
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        transaction.execute(
            "UPDATE brain_sessions SET locked_pid = NULL, last_active_at = ?8
             WHERE brain_instance_id = ?1 AND locked_pid IS NOT NULL
               AND agent_kind = ?2 AND workspace_id = ?3
               AND actor_id = ?4 AND channel = ?5
               AND EXISTS (
                 SELECT 1 FROM receiver_session_registrations
                 WHERE workspace_id = ?3 AND conversation_id = ?6
                   AND agent_kind = ?2 AND actor_id = ?4 AND channel = ?5
                   AND brain_instance_id = ?1 AND registered_session_id = ?7
               )",
            rusqlite::params![
                registration.instance(),
                scope.agent_kind().as_str(),
                self.workspace_id,
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                registration.conversation_id().to_string(),
                registration.registered_session().as_str(),
                self.now(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
               AND brain_instance_id = ?6 AND registered_session_id = ?7",
            rusqlite::params![
                self.workspace_id,
                registration.conversation_id().to_string(),
                scope.agent_kind().as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                registration.instance(),
                registration.registered_session().as_str(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Replace only the native binding after an exact registered instance rotates.
    pub fn replace_receiver_binding_from_instance(
        &self,
        registration: &ReceiverSessionAttribution,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        self.validate_receiver_session_scope(registration.conversation_id(), registration.scope())?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if !replace_receiver_binding_in_transaction(
            &transaction,
            &self.workspace_id,
            registration,
            ReceiverBindingTarget::Current,
            observed_at_unix_ms,
        )? {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }

    fn validate_receiver_session_scope(
        &self,
        conversation_id: ReceiverConversationId,
        scope: &SessionScope,
    ) -> Result<()> {
        anyhow::ensure!(
            scope.workspace_id().to_string() == self.workspace_id,
            "receiver session scope belongs to another workspace"
        );
        let exists = self.conn.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM receiver_conversations
               WHERE workspace_id = ?1 AND conversation_id = ?2
                 AND user_id = ?3 AND channel = ?4
             )",
            rusqlite::params![
                self.workspace_id,
                conversation_id.to_string(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
            |row| row.get::<_, bool>(0),
        )?;
        anyhow::ensure!(
            exists,
            "receiver session scope does not match its conversation"
        );
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) enum ReceiverBindingTarget<'session> {
    Current,
    ExactCompleted(&'session AgentSession),
}

pub(super) fn replace_receiver_binding_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    registration: &ReceiverSessionAttribution,
    target: ReceiverBindingTarget<'_>,
    observed_at_unix_ms: u64,
) -> Result<bool> {
    let scope = registration.scope();
    let exact_completed_session = match target {
        ReceiverBindingTarget::Current => None,
        ReceiverBindingTarget::ExactCompleted(session) => Some(session.as_str()),
    };
    let native_session = transaction
        .query_row(
            "SELECT active.agent_session_id,
                    COALESCE(conversation.agent_kind = registration.agent_kind
                      AND conversation.agent_session_id = active.agent_session_id, FALSE)
                 FROM receiver_session_registrations AS registration
                 JOIN brain_sessions AS active
                   ON active.brain_instance_id = registration.brain_instance_id
                  AND active.agent_kind = registration.agent_kind
                  AND active.workspace_id = registration.workspace_id
                  AND active.actor_id = registration.actor_id
                  AND active.channel = registration.channel
                  AND active.locked_pid IS NOT NULL
                 JOIN receiver_conversations AS conversation
                   ON conversation.workspace_id = registration.workspace_id
                  AND conversation.conversation_id = registration.conversation_id
                  AND conversation.user_id = registration.actor_id
                  AND conversation.channel = registration.channel
                 WHERE registration.workspace_id = ?1
                   AND registration.conversation_id = ?2
                   AND registration.agent_kind = ?3
                   AND registration.actor_id = ?4
                   AND registration.channel = ?5
                   AND registration.brain_instance_id = ?6
                   AND registration.registered_session_id = ?7
                   AND (?8 IS NULL OR (
                     active.agent_session_id = ?8
                     AND active.completion_status = 'completed'
                   ))",
            rusqlite::params![
                workspace_id,
                registration.conversation_id().to_string(),
                scope.agent_kind().as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                registration.instance(),
                registration.registered_session().as_str(),
                exact_completed_session,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?;
    let Some((native_session, confirms_resume)) = native_session else {
        return Ok(false);
    };
    if native_session == registration.registered_session().as_str()
        && scope.agent_kind() != crate::agent::AgentKind::Claude
        && !confirms_resume
    {
        return Ok(false);
    }
    let native_session = AgentSession::new(native_session)?;
    let registration_changed = transaction.execute(
        "UPDATE receiver_session_registrations SET actual_session_id = ?8
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
               AND brain_instance_id = ?6 AND registered_session_id = ?7",
        rusqlite::params![
            workspace_id,
            registration.conversation_id().to_string(),
            scope.agent_kind().as_str(),
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            registration.instance(),
            registration.registered_session().as_str(),
            native_session.as_str(),
        ],
    )?;
    if registration_changed != 1 {
        return Ok(false);
    }
    let conversation_changed = transaction.execute(
        "UPDATE receiver_conversations
             SET agent_kind = ?3, agent_session_id = ?4, updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND user_id = ?6 AND channel = ?7",
        rusqlite::params![
            workspace_id,
            registration.conversation_id().to_string(),
            scope.agent_kind().as_str(),
            native_session.as_str(),
            to_i64(observed_at_unix_ms, "receiver binding observation time")?,
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
        ],
    )?;
    if conversation_changed != 1 {
        return Ok(false);
    }
    Ok(true)
}

fn insert_registration(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    conversation_id: ReceiverConversationId,
    session: &AgentSession,
    instance: &str,
    scope: &SessionScope,
) -> Result<()> {
    connection.execute(
        "INSERT INTO receiver_session_registrations
           (workspace_id, conversation_id, agent_kind, actor_id, channel,
            brain_instance_id, registered_session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            workspace_id,
            conversation_id.to_string(),
            scope.agent_kind().as_str(),
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            instance,
            session.as_str(),
        ],
    )?;
    Ok(())
}
