//! Exact session ownership for one isolated receiver process.

use crate::agent::{AgentSession, SessionScope, SessionStore};

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
pub(crate) struct ReceiverSessionRegistration<'store, Store: SessionStore> {
    store: &'store Store,
    instance: &'store str,
    armed: bool,
}

impl<'store, Store: SessionStore> ReceiverSessionRegistration<'store, Store> {
    pub(crate) fn register_fresh(
        store: &'store Store,
        remote: &'store ReceiverRemoteSession,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Self> {
        SessionStore::register(store, remote.placeholder(), remote.instance(), pid, scope)?;
        Ok(Self {
            store,
            instance: remote.instance(),
            armed: true,
        })
    }

    pub(crate) fn claim_resume(
        store: &'store Store,
        remote: &'store ReceiverRemoteSession,
        session: &AgentSession,
        pid: i32,
        scope: &SessionScope,
    ) -> anyhow::Result<Option<Self>> {
        if !SessionStore::claim(store, session, remote.instance(), pid, scope)? {
            return Ok(None);
        }
        Ok(Some(Self {
            store,
            instance: remote.instance(),
            armed: true,
        }))
    }

    pub(crate) fn commit(mut self) {
        self.armed = false;
    }
}

impl<Store: SessionStore> Drop for ReceiverSessionRegistration<'_, Store> {
    fn drop(&mut self) {
        if self.armed {
            let _ = SessionStore::release(self.store, self.instance);
        }
    }
}
