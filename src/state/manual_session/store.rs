use std::error::Error;
use std::fmt::{Display, Formatter};

use anyhow::Result;

use crate::agent::{AgentSession, SessionScope};
use crate::manual_session::{ManualSessionId, ManualSessionRecord, ManualSessionRole};
use crate::state::Db;

use self::sql::{
    compact_later_positions, decode_record, immediate_transaction, insert_brain_session,
    insert_mapping, load_mapping, release_native_session,
};

mod sql;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManualSessionStoreError {
    MainCannotClose,
    MappingNotFound,
    NativeSessionNotClaimed,
    ScopeMismatch,
}

impl Display for ManualSessionStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MainCannotClose => "the main manual session cannot be closed",
            Self::MappingNotFound => "manual session mapping was not found in this scope",
            Self::NativeSessionNotClaimed => {
                "manual session does not own the exact claimed native session"
            }
            Self::ScopeMismatch => "manual session scope belongs to another workspace",
        })
    }
}

impl Error for ManualSessionStoreError {}

impl Db {
    pub(crate) fn manual_sessions(&self, scope: &SessionScope) -> Result<Vec<ManualSessionRecord>> {
        self.validate_manual_session_scope(scope)?;
        let mut statement = self.conn.prepare(
            "SELECT manual_session_id, agent_session_id, title, position, role
             FROM manual_sessions
             WHERE agent_kind = ?1 AND workspace_id = ?2
               AND actor_id = ?3 AND channel = ?4
             ORDER BY CASE role WHEN 'main' THEN 0 ELSE 1 END, position",
        )?;
        let rows = statement.query_map(
            rusqlite::params![
                scope.agent_kind().as_str(),
                self.workspace_id.as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )?;
        rows.map(|row| decode_record(row?)).collect()
    }

    pub(crate) fn register_fresh_manual_session(
        &self,
        record: &ManualSessionRecord,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        self.validate_manual_session_scope(scope)?;
        let transaction = immediate_transaction(&self.conn)?;
        insert_brain_session(
            &transaction,
            record,
            pid,
            scope,
            &self.workspace_id,
            self.now(),
        )?;
        insert_mapping(&transaction, record, scope, &self.workspace_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn attach_manual_session(
        &self,
        record: &ManualSessionRecord,
        scope: &SessionScope,
    ) -> Result<()> {
        self.validate_manual_session_scope(scope)?;
        let transaction = immediate_transaction(&self.conn)?;
        let claimed: bool = transaction.query_row(
            "SELECT EXISTS (
               SELECT 1 FROM brain_sessions
               WHERE agent_kind = ?1 AND agent_session_id = ?2
                 AND brain_instance_id = ?3 AND locked_pid IS NOT NULL
                 AND workspace_id = ?4 AND actor_id = ?5 AND channel = ?6
             )",
            rusqlite::params![
                scope.agent_kind().as_str(),
                record.agent_session().as_str(),
                record.id().as_str(),
                self.workspace_id.as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
            |row| row.get(0),
        )?;
        if !claimed {
            return Err(ManualSessionStoreError::NativeSessionNotClaimed.into());
        }
        insert_mapping(&transaction, record, scope, &self.workspace_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn replace_manual_session(
        &self,
        id: &ManualSessionId,
        session: &AgentSession,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<ManualSessionRecord> {
        self.validate_manual_session_scope(scope)?;
        let transaction = immediate_transaction(&self.conn)?;
        let current = load_mapping(&transaction, id, scope, &self.workspace_id)?
            .ok_or(ManualSessionStoreError::MappingNotFound)?;
        let replacement = ManualSessionRecord {
            id: id.clone(),
            agent_session: session.clone(),
            name: current.name.clone(),
            position: current.position,
            role: current.role,
        };
        insert_brain_session(
            &transaction,
            &replacement,
            pid,
            scope,
            &self.workspace_id,
            self.now(),
        )?;
        let updated = transaction.execute(
            "UPDATE manual_sessions SET agent_session_id = ?1
             WHERE manual_session_id = ?2 AND agent_kind = ?3
               AND agent_session_id = ?4 AND workspace_id = ?5
               AND actor_id = ?6 AND channel = ?7",
            rusqlite::params![
                session.as_str(),
                id.as_str(),
                scope.agent_kind().as_str(),
                current.agent_session.as_str(),
                self.workspace_id.as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
        if updated != 1 {
            return Err(ManualSessionStoreError::MappingNotFound.into());
        }
        let released = release_native_session(
            &transaction,
            id,
            &current.agent_session,
            scope,
            &self.workspace_id,
            self.now(),
        )?;
        if released != 1 {
            return Err(ManualSessionStoreError::NativeSessionNotClaimed.into());
        }
        transaction.commit()?;
        Ok(replacement)
    }

    pub(crate) fn close_manual_session(
        &self,
        id: &ManualSessionId,
        scope: &SessionScope,
    ) -> Result<()> {
        self.validate_manual_session_scope(scope)?;
        let transaction = immediate_transaction(&self.conn)?;
        let current = load_mapping(&transaction, id, scope, &self.workspace_id)?
            .ok_or(ManualSessionStoreError::MappingNotFound)?;
        if current.role == ManualSessionRole::Main {
            return Err(ManualSessionStoreError::MainCannotClose.into());
        }
        let deleted = transaction.execute(
            "DELETE FROM manual_sessions
             WHERE manual_session_id = ?1 AND agent_kind = ?2
               AND agent_session_id = ?3 AND workspace_id = ?4
               AND actor_id = ?5 AND channel = ?6 AND role = 'additional'",
            rusqlite::params![
                id.as_str(),
                scope.agent_kind().as_str(),
                current.agent_session.as_str(),
                self.workspace_id.as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
        if deleted != 1 {
            return Err(ManualSessionStoreError::MappingNotFound.into());
        }
        let released = release_native_session(
            &transaction,
            id,
            &current.agent_session,
            scope,
            &self.workspace_id,
            self.now(),
        )?;
        if released != 1 {
            return Err(ManualSessionStoreError::NativeSessionNotClaimed.into());
        }
        compact_later_positions(&transaction, current.position, scope, &self.workspace_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn release_manual_session(
        &self,
        id: &ManualSessionId,
        scope: &SessionScope,
    ) -> Result<()> {
        self.validate_manual_session_scope(scope)?;
        let transaction = immediate_transaction(&self.conn)?;
        let current = load_mapping(&transaction, id, scope, &self.workspace_id)?
            .ok_or(ManualSessionStoreError::MappingNotFound)?;
        let released = release_native_session(
            &transaction,
            id,
            &current.agent_session,
            scope,
            &self.workspace_id,
            self.now(),
        )?;
        if released != 1 {
            return Err(ManualSessionStoreError::NativeSessionNotClaimed.into());
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn rollback_fresh_manual_session(
        &self,
        record: &ManualSessionRecord,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        self.validate_manual_session_scope(scope)?;
        let transaction = immediate_transaction(&self.conn)?;
        let current = load_mapping(&transaction, record.id(), scope, &self.workspace_id)?
            .ok_or(ManualSessionStoreError::MappingNotFound)?;
        if current.role == ManualSessionRole::Main {
            return Err(ManualSessionStoreError::MainCannotClose.into());
        }
        anyhow::ensure!(current == *record, "fresh manual session mapping changed");
        transaction.execute(
            "DELETE FROM manual_sessions
             WHERE manual_session_id = ?1 AND agent_kind = ?2
               AND agent_session_id = ?3 AND workspace_id = ?4
               AND actor_id = ?5 AND channel = ?6 AND role = 'additional'",
            rusqlite::params![
                record.id.as_str(),
                scope.agent_kind().as_str(),
                record.agent_session.as_str(),
                self.workspace_id.as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM brain_sessions
             WHERE agent_kind = ?1 AND agent_session_id = ?2
               AND brain_instance_id = ?3 AND workspace_id = ?4
               AND actor_id = ?5 AND channel = ?6 AND locked_pid = ?7
               AND source = 'fresh'",
            rusqlite::params![
                scope.agent_kind().as_str(),
                record.agent_session.as_str(),
                record.id.as_str(),
                self.workspace_id.as_str(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                pid,
            ],
        )?;
        anyhow::ensure!(deleted == 1, "fresh manual session ownership changed");
        compact_later_positions(&transaction, current.position, scope, &self.workspace_id)?;
        transaction.commit()?;
        Ok(())
    }

    fn validate_manual_session_scope(&self, scope: &SessionScope) -> Result<()> {
        if scope.workspace_id().to_string() != self.workspace_id
            || scope.actor().channel().as_str() != "interactive"
        {
            return Err(ManualSessionStoreError::ScopeMismatch.into());
        }
        Ok(())
    }
}
