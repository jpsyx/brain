//! Exact session ownership for one isolated receiver process.

use crate::agent::{AgentSession, SessionScope, SessionStore};
use crate::state::{Db, ReceiverConversationId, ReceiverSessionAttribution};
use crate::tui::state::AppServices;

pub(crate) trait ReceiverSessionStore: SessionStore {
    fn register_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<ReceiverSessionAttribution>;

    fn claim_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Option<ReceiverSessionAttribution>>;

    fn release_receiver_session(
        &self,
        registration: &ReceiverSessionAttribution,
    ) -> anyhow::Result<()>;
}

impl ReceiverSessionStore for Db {
    fn register_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<ReceiverSessionAttribution> {
        Self::register_receiver_session(self, conversation_id, session, instance, pid, scope)
    }

    fn claim_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Option<ReceiverSessionAttribution>> {
        Self::claim_receiver_session(self, conversation_id, session, instance, pid, scope)
    }

    fn release_receiver_session(
        &self,
        registration: &ReceiverSessionAttribution,
    ) -> anyhow::Result<()> {
        Self::release_receiver_session(self, registration)
    }
}

impl ReceiverSessionStore for AppServices {
    fn register_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<ReceiverSessionAttribution> {
        Self::register_receiver_session(self, conversation_id, session, instance, pid, scope)
    }

    fn claim_receiver_session(
        &self,
        conversation_id: ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Option<ReceiverSessionAttribution>> {
        Self::claim_receiver_session(self, conversation_id, session, instance, pid, scope)
    }

    fn release_receiver_session(
        &self,
        registration: &ReceiverSessionAttribution,
    ) -> anyhow::Result<()> {
        Self::release_receiver_session(self, registration)
    }
}

/// Fresh hook lineage and placeholder for one isolated receiver process.
pub(crate) struct ReceiverRemoteSession {
    instance: String,
    placeholder: AgentSession,
}

impl ReceiverRemoteSession {
    pub(crate) fn new(interactive_instance: &str) -> Self {
        loop {
            let id = uuid::Uuid::new_v4();
            let instance = format!("receiver-run-{id}");
            if instance == interactive_instance {
                continue;
            }
            let placeholder = AgentSession::new(format!("pending-receiver-{id}"))
                .expect("a UUID-backed receiver placeholder is non-blank");
            return Self {
                instance,
                placeholder,
            };
        }
    }

    pub(crate) fn instance(&self) -> &str {
        &self.instance
    }

    pub(crate) const fn placeholder(&self) -> &AgentSession {
        &self.placeholder
    }
}

/// Releases an exact remote owner unless its controller accepted ownership.
pub(crate) struct ReceiverSessionRegistration<'store, Store: ReceiverSessionStore> {
    store: &'store Store,
    attribution: ReceiverSessionAttribution,
    armed: bool,
}

impl<'store, Store: ReceiverSessionStore> ReceiverSessionRegistration<'store, Store> {
    pub(crate) fn register_fresh(
        store: &'store Store,
        conversation_id: ReceiverConversationId,
        remote: &ReceiverRemoteSession,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Self> {
        let attribution = ReceiverSessionStore::register_receiver_session(
            store,
            conversation_id,
            remote.placeholder(),
            remote.instance(),
            pid,
            scope,
        )?;
        Ok(Self {
            store,
            attribution,
            armed: true,
        })
    }

    pub(crate) fn claim_resume(
        store: &'store Store,
        conversation_id: ReceiverConversationId,
        remote: &ReceiverRemoteSession,
        session: &AgentSession,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Option<Self>> {
        let Some(attribution) = ReceiverSessionStore::claim_receiver_session(
            store,
            conversation_id,
            session,
            remote.instance(),
            pid,
            scope,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            store,
            attribution,
            armed: true,
        }))
    }

    pub(crate) fn commit(mut self) -> ReceiverSessionAttribution {
        self.armed = false;
        self.attribution.clone()
    }

    pub(crate) fn cleanup(mut self) -> anyhow::Result<()> {
        ReceiverSessionStore::release_receiver_session(self.store, &self.attribution)?;
        self.armed = false;
        Ok(())
    }
}

impl<Store: ReceiverSessionStore> Drop for ReceiverSessionRegistration<'_, Store> {
    fn drop(&mut self) {
        if self.armed {
            let _ = ReceiverSessionStore::release_receiver_session(self.store, &self.attribution);
        }
    }
}
