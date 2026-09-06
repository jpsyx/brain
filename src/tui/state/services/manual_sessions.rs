use anyhow::Result;

use crate::agent::{AgentSession, SessionScope};
use crate::manual_session::{ManualSessionId, ManualSessionRecord};

use super::AppServices;

impl AppServices {
    pub(crate) fn manual_sessions(&self, scope: &SessionScope) -> Result<Vec<ManualSessionRecord>> {
        self.db.manual_sessions(scope)
    }

    pub(crate) fn register_fresh_manual_session(
        &self,
        record: &ManualSessionRecord,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        self.db.register_fresh_manual_session(record, pid, scope)
    }

    pub(crate) fn attach_manual_session(
        &self,
        record: &ManualSessionRecord,
        scope: &SessionScope,
    ) -> Result<()> {
        self.db.attach_manual_session(record, scope)
    }

    pub(crate) fn replace_manual_session(
        &self,
        id: &ManualSessionId,
        session: &AgentSession,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<ManualSessionRecord> {
        self.db.replace_manual_session(id, session, pid, scope)
    }

    pub(crate) fn close_manual_session(
        &self,
        id: &ManualSessionId,
        scope: &SessionScope,
    ) -> Result<()> {
        self.db.close_manual_session(id, scope)
    }

    pub(crate) fn release_manual_session(
        &self,
        id: &ManualSessionId,
        scope: &SessionScope,
    ) -> Result<()> {
        self.db.release_manual_session(id, scope)
    }

    pub(crate) fn rollback_fresh_manual_session(
        &self,
        record: &ManualSessionRecord,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        self.db.rollback_fresh_manual_session(record, pid, scope)
    }
}
