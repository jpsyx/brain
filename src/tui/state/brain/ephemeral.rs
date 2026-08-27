use crate::agent::{
    AgentController, AgentError, AgentObservationError, AgentObservationRequest,
    AgentObservationResult,
};
use crate::skill_session::SkillSessionKey;
use crate::state::ReceiverJobId;
use crate::tui::model::SessionTabId;

struct SkillSessionMetadata {
    key: SkillSessionKey,
    token: String,
}

struct ReceiverRunMetadata {
    job_id: ReceiverJobId,
    instance: String,
}

enum EphemeralTabMetadata {
    SkillSession(SkillSessionMetadata),
    ReceiverRun(ReceiverRunMetadata),
}

struct EphemeralTab {
    id: SessionTabId,
    title: String,
    metadata: EphemeralTabMetadata,
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
pub(super) struct EphemeralTabs {
    tabs: Vec<EphemeralTab>,
    next_id: u32,
}

impl EphemeralTabs {
    pub(super) fn has_skill_sessions(&self) -> bool {
        self.tabs
            .iter()
            .any(|tab| matches!(&tab.metadata, EphemeralTabMetadata::SkillSession(_)))
    }

    pub(super) fn ids(&self) -> Vec<SessionTabId> {
        self.tabs.iter().map(|tab| tab.id).collect()
    }

    pub(super) fn skill_session_ids(&self) -> Vec<SessionTabId> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                EphemeralTabMetadata::SkillSession(_) => Some(tab.id),
                EphemeralTabMetadata::ReceiverRun(_) => None,
            })
            .collect()
    }

    pub(super) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                EphemeralTabMetadata::SkillSession(session) => Some(session.key),
                EphemeralTabMetadata::ReceiverRun(_) => None,
            })
            .collect()
    }

    pub(super) fn skill_session_rows(&self) -> Vec<(SkillSessionKey, String)> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                EphemeralTabMetadata::SkillSession(session) => {
                    Some((session.key, tab.title.clone()))
                }
                EphemeralTabMetadata::ReceiverRun(_) => None,
            })
            .collect()
    }

    pub(super) fn skill_session_observations(&self) -> Vec<SkillSessionObservation> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                EphemeralTabMetadata::SkillSession(session) => Some(SkillSessionObservation {
                    id: tab.id,
                    title: tab.title.clone(),
                    token: session.token.clone(),
                    exited: tab.controller.is_alive().is_ok_and(|alive| !alive),
                }),
                EphemeralTabMetadata::ReceiverRun(_) => None,
            })
            .collect()
    }

    pub(super) fn receiver_run_observations(&self) -> Vec<ReceiverRunObservation> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                EphemeralTabMetadata::ReceiverRun(receiver) => Some(ReceiverRunObservation {
                    id: tab.id,
                    job_id: receiver.job_id,
                    instance: receiver.instance.clone(),
                    exited: tab.controller.is_alive().is_ok_and(|alive| !alive),
                }),
                EphemeralTabMetadata::SkillSession(_) => None,
            })
            .collect()
    }

    pub(super) fn poll_receiver_run(
        &self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
        request: &AgentObservationRequest,
    ) -> Result<ReceiverRunPoll, ReceiverRunPollError> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .ok_or(ReceiverRunPollError::MissingTab)?;
        let EphemeralTabMetadata::ReceiverRun(receiver) = &tab.metadata else {
            return Err(ReceiverRunPollError::IdentityMismatch);
        };
        if receiver.job_id != job_id || receiver.instance != instance {
            return Err(ReceiverRunPollError::IdentityMismatch);
        }
        let exited = tab.controller.is_alive().is_ok_and(|alive| !alive);
        let observation = tab
            .controller
            .observe(request)
            .map_err(ReceiverRunPollError::Observation)?;
        Ok(ReceiverRunPoll {
            exited,
            observation,
        })
    }

    pub(super) fn skill_session_id(&self, key: SkillSessionKey) -> Option<SessionTabId> {
        self.tabs
            .iter()
            .find(|tab| {
                matches!(
                    &tab.metadata,
                    EphemeralTabMetadata::SkillSession(session) if session.key == key
                )
            })
            .map(|tab| tab.id)
    }

    pub(super) fn is_skill_session(&self, id: SessionTabId) -> bool {
        self.tabs.iter().any(|tab| {
            tab.id == id && matches!(&tab.metadata, EphemeralTabMetadata::SkillSession(_))
        })
    }

    #[cfg(test)]
    pub(super) fn skill_session_token(&self, key: SkillSessionKey) -> Option<String> {
        self.tabs.iter().find_map(|tab| match &tab.metadata {
            EphemeralTabMetadata::SkillSession(session) if session.key == key => {
                Some(session.token.clone())
            }
            EphemeralTabMetadata::SkillSession(_) | EphemeralTabMetadata::ReceiverRun(_) => None,
        })
    }

    pub(super) fn add_skill_session(
        &mut self,
        key: SkillSessionKey,
        title: String,
        token: String,
        controller: AgentController,
    ) -> Result<SessionTabId, SkillSessionTabIdExhausted> {
        self.add(
            title,
            EphemeralTabMetadata::SkillSession(SkillSessionMetadata { key, token }),
            controller,
        )
        .map_err(|SessionTabIdExhausted| SkillSessionTabIdExhausted)
    }

    pub(super) fn add_receiver_run(
        &mut self,
        job_id: ReceiverJobId,
        title: String,
        instance: String,
        controller: AgentController,
    ) -> Result<SessionTabId, ReceiverRunTabError> {
        let reservation = match self.reserve_receiver_run() {
            Ok(reservation) => reservation,
            Err(error) => {
                let mut controller = controller;
                let _ = controller.shutdown();
                return Err(error);
            }
        };
        Ok(self.insert_receiver_run(&reservation, job_id, title, instance, controller))
    }

    pub(super) fn reserve_receiver_run(
        &self,
    ) -> Result<ReceiverRunReservation, ReceiverRunTabError> {
        if self
            .tabs
            .iter()
            .any(|tab| matches!(&tab.metadata, EphemeralTabMetadata::ReceiverRun(_)))
        {
            return Err(ReceiverRunTabError::AlreadyRunning);
        }
        let Some(next_id) = self.next_id.checked_add(1) else {
            return Err(ReceiverRunTabError::IdExhausted);
        };
        Ok(ReceiverRunReservation {
            id: SessionTabId(self.next_id),
            next_id,
        })
    }

    pub(super) fn insert_receiver_run(
        &mut self,
        reservation: &ReceiverRunReservation,
        job_id: ReceiverJobId,
        title: String,
        instance: String,
        controller: AgentController,
    ) -> SessionTabId {
        assert_eq!(reservation.id, SessionTabId(self.next_id));
        assert!(
            !self
                .tabs
                .iter()
                .any(|tab| matches!(&tab.metadata, EphemeralTabMetadata::ReceiverRun(_)))
        );
        self.tabs.push(EphemeralTab {
            id: reservation.id,
            title,
            metadata: EphemeralTabMetadata::ReceiverRun(ReceiverRunMetadata { job_id, instance }),
            controller,
        });
        self.next_id = reservation.next_id;
        reservation.id
    }

    pub(super) fn remove_skill_session(&mut self, id: SessionTabId) -> Option<RemovedSkillSession> {
        let index = self.tabs.iter().position(|tab| {
            tab.id == id && matches!(&tab.metadata, EphemeralTabMetadata::SkillSession(_))
        })?;
        let mut tab = self.tabs.remove(index);
        let _ = tab.controller.shutdown();
        let EphemeralTabMetadata::SkillSession(session) = tab.metadata else {
            unreachable!("the located tab was a skill session")
        };
        Some(RemovedSkillSession {
            token: session.token,
        })
    }

    pub(super) fn remove_receiver_run(&mut self, id: SessionTabId) -> Option<RemovedReceiverRun> {
        let index = self.tabs.iter().position(|tab| {
            tab.id == id && matches!(&tab.metadata, EphemeralTabMetadata::ReceiverRun(_))
        })?;
        let mut tab = self.tabs.remove(index);
        let _ = tab.controller.shutdown();
        let EphemeralTabMetadata::ReceiverRun(receiver) = tab.metadata else {
            unreachable!("the located tab was a receiver run")
        };
        Some(RemovedReceiverRun {
            job_id: receiver.job_id,
            instance: receiver.instance,
        })
    }

    pub(super) fn shutdown_receiver_run(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Result<bool, AgentError> {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return Ok(false);
        };
        let EphemeralTabMetadata::ReceiverRun(receiver) = &tab.metadata else {
            return Ok(false);
        };
        if receiver.job_id != job_id || receiver.instance != instance {
            return Ok(false);
        }
        tab.controller.shutdown()?;
        Ok(true)
    }

    pub(super) fn remove_shutdown_receiver_run(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Option<RemovedReceiverRun> {
        let index = self.tabs.iter().position(|tab| {
            tab.id == id
                && matches!(
                    &tab.metadata,
                    EphemeralTabMetadata::ReceiverRun(receiver)
                        if receiver.job_id == job_id && receiver.instance == instance
                )
        })?;
        let tab = self.tabs.remove(index);
        let EphemeralTabMetadata::ReceiverRun(receiver) = tab.metadata else {
            unreachable!("the located tab was an exact receiver run")
        };
        Some(RemovedReceiverRun {
            job_id: receiver.job_id,
            instance: receiver.instance,
        })
    }

    fn add(
        &mut self,
        title: String,
        metadata: EphemeralTabMetadata,
        mut controller: AgentController,
    ) -> Result<SessionTabId, SessionTabIdExhausted> {
        let Some(next_id) = self.next_id.checked_add(1) else {
            let _ = controller.shutdown();
            return Err(SessionTabIdExhausted);
        };
        let id = SessionTabId(self.next_id);
        self.tabs.push(EphemeralTab {
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

    #[cfg(test)]
    pub(super) fn receiver_run_controller(&self, id: SessionTabId) -> Option<&AgentController> {
        self.tabs.iter().find_map(|tab| match &tab.metadata {
            EphemeralTabMetadata::ReceiverRun(_) if tab.id == id => Some(&tab.controller),
            EphemeralTabMetadata::ReceiverRun(_) | EphemeralTabMetadata::SkillSession(_) => None,
        })
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
