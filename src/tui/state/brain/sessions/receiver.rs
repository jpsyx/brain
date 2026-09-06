use crate::agent::{AgentController, AgentError, AgentObservationRequest};
use crate::state::ReceiverJobId;
use crate::tui::model::SessionTabId;

use super::{
    ReceiverRunObservation, ReceiverRunPoll, ReceiverRunPollError, ReceiverRunReservation,
    ReceiverRunTabError, ReceiverSessionMetadata, RemovedReceiverRun, SessionTab, SessionTabKind,
    SessionTabs,
};

impl SessionTabs {
    pub(in crate::tui::state::brain) fn is_receiver_session(&self, id: SessionTabId) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.id == id && matches!(tab.metadata, SessionTabKind::Receiver(_)))
    }

    pub(in crate::tui::state::brain) fn receiver_run_observations(
        &self,
    ) -> Vec<ReceiverRunObservation> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                SessionTabKind::Receiver(receiver) => Some(ReceiverRunObservation {
                    id: tab.id,
                    job_id: receiver.job_id,
                    instance: receiver.instance.clone(),
                    exited: tab.controller.is_alive().is_ok_and(|alive| !alive),
                }),
                SessionTabKind::Manual(_) | SessionTabKind::Skill(_) => None,
            })
            .collect()
    }

    pub(in crate::tui::state::brain) fn poll_receiver_run(
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
        let SessionTabKind::Receiver(receiver) = &tab.metadata else {
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

    pub(in crate::tui::state::brain) fn add_receiver_run(
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

    pub(in crate::tui::state::brain) fn reserve_receiver_run(
        &self,
    ) -> Result<ReceiverRunReservation, ReceiverRunTabError> {
        if self
            .tabs
            .iter()
            .any(|tab| matches!(&tab.metadata, SessionTabKind::Receiver(_)))
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

    pub(in crate::tui::state::brain) fn insert_receiver_run(
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
                .any(|tab| matches!(&tab.metadata, SessionTabKind::Receiver(_)))
        );
        self.tabs.push(SessionTab {
            id: reservation.id,
            title,
            metadata: SessionTabKind::Receiver(ReceiverSessionMetadata { job_id, instance }),
            controller,
        });
        self.next_id = reservation.next_id;
        reservation.id
    }

    pub(in crate::tui::state::brain) fn remove_receiver_run(
        &mut self,
        id: SessionTabId,
    ) -> Option<RemovedReceiverRun> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id && matches!(&tab.metadata, SessionTabKind::Receiver(_)))?;
        let mut tab = self.tabs.remove(index);
        let _ = tab.controller.shutdown();
        let SessionTabKind::Receiver(receiver) = tab.metadata else {
            unreachable!("the located tab was a receiver run")
        };
        Some(RemovedReceiverRun {
            job_id: receiver.job_id,
            instance: receiver.instance,
        })
    }

    pub(in crate::tui::state::brain) fn shutdown_receiver_run(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Result<bool, AgentError> {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return Ok(false);
        };
        let SessionTabKind::Receiver(receiver) = &tab.metadata else {
            return Ok(false);
        };
        if receiver.job_id != job_id || receiver.instance != instance {
            return Ok(false);
        }
        tab.controller.shutdown()?;
        Ok(true)
    }

    pub(in crate::tui::state::brain) fn remove_shutdown_receiver_run(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Option<RemovedReceiverRun> {
        let index = self.tabs.iter().position(|tab| {
            tab.id == id
                && matches!(
                    &tab.metadata,
                    SessionTabKind::Receiver(receiver)
                        if receiver.job_id == job_id && receiver.instance == instance
                )
        })?;
        let tab = self.tabs.remove(index);
        let SessionTabKind::Receiver(receiver) = tab.metadata else {
            unreachable!("the located tab was an exact receiver run")
        };
        Some(RemovedReceiverRun {
            job_id: receiver.job_id,
            instance: receiver.instance,
        })
    }

    pub(in crate::tui::state::brain) fn detach_receiver_run_controller(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Option<AgentController> {
        let index = self.tabs.iter().position(|tab| {
            tab.id == id
                && matches!(
                    &tab.metadata,
                    SessionTabKind::Receiver(receiver)
                        if receiver.job_id == job_id && receiver.instance == instance
                )
        })?;
        let tab = self.tabs.remove(index);
        Some(tab.controller)
    }

    #[cfg(test)]
    pub(in crate::tui::state::brain) fn receiver_run_controller(
        &self,
        id: SessionTabId,
    ) -> Option<&AgentController> {
        self.tabs.iter().find_map(|tab| match &tab.metadata {
            SessionTabKind::Receiver(_) if tab.id == id => Some(&tab.controller),
            SessionTabKind::Receiver(_) | SessionTabKind::Manual(_) | SessionTabKind::Skill(_) => {
                None
            }
        })
    }
}
