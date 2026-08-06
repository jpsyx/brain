use super::{CompletionStatus, Db, SessionScope};
use crate::agent::{AgentSession, SessionStore};
use anyhow::Result;

impl SessionStore for Db {
    fn reap_dead_locks(&self) -> Result<()> {
        Self::reap_dead_locks(self)
    }

    fn sessions_by_recency(&self, scope: &SessionScope) -> Vec<String> {
        Self::sessions_by_recency(self, scope)
    }

    fn claim(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<bool> {
        Self::claim(self, session.as_str(), instance, pid, scope)
    }

    fn register(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        Self::register_scoped_fresh(self, session.as_str(), instance, pid, scope)
    }

    fn release(&self, instance: &str) -> Result<()> {
        Self::release(self, instance)
    }

    fn mark_active(&self, instance: &str, scope: &SessionScope) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE brain_sessions SET completion_status = ?1
             WHERE brain_instance_id = ?2 AND locked_pid IS NOT NULL
               AND agent_kind = ?3 AND workspace_id = ?4
               AND actor_id = ?5 AND channel = ?6",
            rusqlite::params![
                CompletionStatus::Active.as_str(),
                instance,
                scope.agent_kind().as_str(),
                scope.workspace_id().to_string(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
        Ok(changed == 1)
    }

    fn mark_completed(&self, session: &AgentSession, scope: &SessionScope) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE brain_sessions SET completion_status = ?1
             WHERE agent_kind = ?2 AND agent_session_id = ?3
               AND workspace_id = ?4 AND actor_id = ?5 AND channel = ?6",
            rusqlite::params![
                CompletionStatus::Completed.as_str(),
                scope.agent_kind().as_str(),
                session.as_str(),
                scope.workspace_id().to_string(),
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
        Ok(changed == 1)
    }

    fn completion_status(
        &self,
        session: &AgentSession,
        scope: &SessionScope,
    ) -> Option<CompletionStatus> {
        self.conn
            .query_row(
                "SELECT completion_status FROM brain_sessions
                 WHERE agent_kind = ?1 AND agent_session_id = ?2
                   AND workspace_id = ?3 AND actor_id = ?4 AND channel = ?5",
                rusqlite::params![
                    scope.agent_kind().as_str(),
                    session.as_str(),
                    scope.workspace_id().to_string(),
                    scope.actor().user_id().as_str(),
                    scope.actor().channel().as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|status| CompletionStatus::parse(&status))
    }
}
