//! Shared insertion order and kind-specific ownership for additional session tabs.

use crate::agent::{AgentController, AgentError, AgentObservationError, AgentObservationResult};
use crate::manual_session::ManualSessionId;
use crate::skill_session::SkillSessionKey;
use crate::state::ReceiverJobId;
use crate::tui::model::SessionTabId;

mod manual;
mod receiver;
mod skill;

struct SkillSessionMetadata {
    key: SkillSessionKey,
    token: String,
}

struct ReceiverSessionMetadata {
    job_id: ReceiverJobId,
    instance: String,
}

enum SessionTabKind {
    Manual(ManualTabMetadata),
    Skill(SkillSessionMetadata),
    Receiver(ReceiverSessionMetadata),
}

struct ManualTabMetadata {
    id: ManualSessionId,
    resumed_session_id: Option<String>,
    startup: crate::tui::app_brain::launch::arrival::SessionStartup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionPaletteEntry {
    pub(crate) id: SessionTabId,
    pub(crate) title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionRenameEntry {
    pub(crate) id: Option<SessionTabId>,
    pub(crate) title: String,
    pub(crate) renameable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionCloseEntry {
    pub(crate) id: Option<SessionTabId>,
    pub(crate) title: String,
    pub(crate) closeable: bool,
}

impl SessionPaletteEntry {
    pub(crate) fn new(id: SessionTabId, title: &str) -> Self {
        Self {
            id,
            title: title.to_owned(),
        }
    }
}

pub(crate) struct RemovedManualSession {
    pub(crate) id: ManualSessionId,
}

pub(crate) struct ManualSessionObservation {
    pub(crate) id: SessionTabId,
    pub(crate) manual_session_id: ManualSessionId,
    pub(crate) resumed_session_id: Option<String>,
    pub(crate) alive: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ManualSessionTabIdExhausted;

impl std::fmt::Display for ManualSessionTabIdExhausted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("manual-session tab identity exhausted")
    }
}

impl std::error::Error for ManualSessionTabIdExhausted {}

struct SessionTab {
    id: SessionTabId,
    title: String,
    metadata: SessionTabKind,
    controller: AgentController,
}

pub(crate) struct RemovedSkillSession {
    pub(crate) token: String,
}

pub(crate) struct SkillSessionObservation {
    pub(crate) id: SessionTabId,
    pub(crate) title: String,
    pub(crate) token: String,
    pub(crate) exited: bool,
}

pub(crate) struct RemovedReceiverRun {
    pub(crate) job_id: ReceiverJobId,
    pub(crate) instance: String,
}

pub(crate) struct ReceiverRunObservation {
    pub(crate) id: SessionTabId,
    pub(crate) job_id: ReceiverJobId,
    pub(crate) instance: String,
    pub(crate) exited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverRunPollError {
    MissingTab,
    IdentityMismatch,
    Observation(AgentObservationError),
}

pub(crate) struct ReceiverRunPoll {
    pub(crate) exited: bool,
    pub(crate) observation: AgentObservationResult,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SessionTabIdExhausted;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SkillSessionTabIdExhausted;

impl std::fmt::Display for SkillSessionTabIdExhausted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("skill-session tab identity exhausted")
    }
}

impl std::error::Error for SkillSessionTabIdExhausted {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReceiverRunTabError {
    AlreadyRunning,
    IdExhausted,
}

pub(crate) struct ReceiverRunReservation {
    id: SessionTabId,
    next_id: u32,
}

impl std::fmt::Display for ReceiverRunTabError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => formatter.write_str("a receiver run is already active"),
            Self::IdExhausted => formatter.write_str("receiver-run tab identity exhausted"),
        }
    }
}

impl std::error::Error for ReceiverRunTabError {}

#[derive(Default)]
pub(super) struct SessionTabs {
    tabs: Vec<SessionTab>,
    next_id: u32,
}

impl SessionTabs {
    pub(super) fn ids(&self) -> Vec<SessionTabId> {
        self.tabs.iter().map(|tab| tab.id).collect()
    }

    pub(super) fn rename_session_rows(&self) -> Vec<SessionRenameEntry> {
        self.tabs
            .iter()
            .map(|tab| SessionRenameEntry {
                id: Some(tab.id),
                title: tab.title.clone(),
                renameable: matches!(tab.metadata, SessionTabKind::Manual(_)),
            })
            .collect()
    }

    pub(super) fn close_session_rows(&self) -> Vec<SessionCloseEntry> {
        self.tabs
            .iter()
            .map(|tab| SessionCloseEntry {
                id: Some(tab.id),
                title: tab.title.clone(),
                closeable: matches!(
                    tab.metadata,
                    SessionTabKind::Manual(_) | SessionTabKind::Skill(_)
                ),
            })
            .collect()
    }

    fn add(
        &mut self,
        title: String,
        metadata: SessionTabKind,
        mut controller: AgentController,
    ) -> Result<SessionTabId, SessionTabIdExhausted> {
        let Some(next_id) = self.next_id.checked_add(1) else {
            let _ = controller.shutdown();
            return Err(SessionTabIdExhausted);
        };
        let id = SessionTabId(self.next_id);
        self.tabs.push(SessionTab {
            id,
            title,
            metadata,
            controller,
        });
        self.next_id = next_id;
        Ok(id)
    }

    pub(super) fn controller(&self, id: SessionTabId) -> Option<&AgentController> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.controller)
    }

    pub(super) fn controller_mut(&mut self, id: SessionTabId) -> Option<&mut AgentController> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .map(|tab| &mut tab.controller)
    }

    pub(super) fn title(&self, id: SessionTabId) -> Option<&str> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.title.as_str())
    }

    pub(super) fn titles(&self) -> impl Iterator<Item = &str> {
        self.tabs.iter().map(|tab| tab.title.as_str())
    }

    pub(super) fn shutdown_controllers(&mut self) -> Vec<AgentError> {
        self.tabs
            .iter_mut()
            .filter_map(|tab| tab.controller.shutdown().err())
            .collect()
    }

    #[cfg(test)]
    pub(super) const fn set_next_id(&mut self, next_id: u32) {
        self.next_id = next_id;
    }

    #[cfg(test)]
    pub(super) const fn next_id(&self) -> u32 {
        self.next_id
    }
}
