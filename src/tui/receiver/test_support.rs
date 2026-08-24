use crate::agent::{AgentSession, SessionScope, SessionStore};
use crate::state::{Db, ReceiverConversationId, ReceiverSessionAttribution};

use super::ReceiverSessionStore;

pub(super) struct FailingReleaseStore {
    db: Db,
    release_attempts: std::cell::Cell<u32>,
}

impl FailingReleaseStore {
    pub(super) fn new() -> Self {
        Self {
            db: Db::open_in_memory().expect("state DB"),
            release_attempts: std::cell::Cell::new(0),
        }
    }

    pub(super) const fn db(&self) -> &Db {
        &self.db
    }

    pub(super) fn release_attempts(&self) -> u32 {
        self.release_attempts.get()
    }
}

impl SessionStore for FailingReleaseStore {
    fn reap_dead_locks(&self) -> anyhow::Result<()> {
        SessionStore::reap_dead_locks(&self.db)
    }

    fn sessions_by_recency(&self, scope: &SessionScope) -> Vec<String> {
        SessionStore::sessions_by_recency(&self.db, scope)
    }

    fn claim(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<bool> {
        SessionStore::claim(&self.db, session, instance, pid, scope)
    }

    fn register(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<()> {
        SessionStore::register(&self.db, session, instance, pid, scope)
    }

    fn release(&self, _instance: &str) -> anyhow::Result<()> {
        self.release_attempts
            .set(self.release_attempts.get().saturating_add(1));
        anyhow::bail!("exact receiver release failed")
    }

    fn mark_active(&self, instance: &str, scope: &SessionScope) -> anyhow::Result<bool> {
        SessionStore::mark_active(&self.db, instance, scope)
    }

    fn mark_completed(&self, session: &AgentSession, scope: &SessionScope) -> anyhow::Result<bool> {
        SessionStore::mark_completed(&self.db, session, scope)
    }

    fn completion_status(
        &self,
        session: &AgentSession,
        scope: &SessionScope,
    ) -> Option<crate::agent::CompletionStatus> {
        SessionStore::completion_status(&self.db, session, scope)
    }
}

impl ReceiverSessionStore for FailingReleaseStore {
    fn register_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<ReceiverSessionAttribution> {
        self.db
            .register_receiver_session(conversation_id, session, instance, pid, scope)
    }

    fn claim_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Option<ReceiverSessionAttribution>> {
        self.db
            .claim_receiver_session(conversation_id, session, instance, pid, scope)
    }

    fn release_receiver_session(
        &self,
        _registration: &ReceiverSessionAttribution,
    ) -> anyhow::Result<()> {
        self.release_attempts
            .set(self.release_attempts.get().saturating_add(1));
        anyhow::bail!("exact receiver release failed")
    }
}
