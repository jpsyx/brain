use super::{ReceiverJobId, ReceiverJobToken};
use crate::agent::AgentKind;

/// Exact machine-local work retained after an answer releases agent ownership.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiverAnswerCleanup {
    job_id: ReceiverJobId,
    token: ReceiverJobToken,
    instance: String,
    frontend: AgentKind,
    session_released: bool,
    artifacts_removed: bool,
}

impl std::fmt::Debug for ReceiverAnswerCleanup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiverAnswerCleanup(<redacted>)")
    }
}

impl ReceiverAnswerCleanup {
    pub(in crate::state::receiver) fn new(
        job_id: ReceiverJobId,
        token: ReceiverJobToken,
        instance: String,
        frontend: AgentKind,
        session_released: bool,
        artifacts_removed: bool,
    ) -> Self {
        Self {
            job_id,
            token,
            instance,
            frontend,
            session_released,
            artifacts_removed,
        }
    }

    #[must_use]
    pub const fn job_id(&self) -> ReceiverJobId {
        self.job_id
    }

    #[must_use]
    pub const fn token(&self) -> ReceiverJobToken {
        self.token
    }

    #[must_use]
    pub fn instance(&self) -> &str {
        &self.instance
    }

    #[must_use]
    pub const fn frontend(&self) -> AgentKind {
        self.frontend
    }

    #[must_use]
    pub const fn session_released(&self) -> bool {
        self.session_released
    }

    #[must_use]
    pub const fn artifacts_removed(&self) -> bool {
        self.artifacts_removed
    }
}
