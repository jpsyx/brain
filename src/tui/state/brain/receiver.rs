use crate::agent::{AgentController, AgentError};
use crate::state::ReceiverJobId;
use crate::tui::model::SessionTabId;

use super::{
    BrainPanelState, ReceiverRunObservation, ReceiverRunPoll, ReceiverRunPollError,
    ReceiverRunReservation, ReceiverRunTabError, RemovedReceiverRun,
};

impl BrainPanelState {
    #[must_use]
    pub(crate) fn receiver_run_observations(&self) -> Vec<ReceiverRunObservation> {
        self.session_tabs.receiver_run_observations()
    }

    pub(crate) fn poll_receiver_run(
        &self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
        request: &crate::agent::AgentObservationRequest,
    ) -> Result<ReceiverRunPoll, ReceiverRunPollError> {
        self.session_tabs
            .poll_receiver_run(id, job_id, instance, request)
    }

    pub(crate) fn add_receiver_run(
        &mut self,
        job_id: ReceiverJobId,
        title: String,
        instance: String,
        controller: AgentController,
    ) -> Result<SessionTabId, ReceiverRunTabError> {
        self.session_tabs
            .add_receiver_run(job_id, title, instance, controller)
    }

    pub(crate) fn reserve_receiver_run(
        &self,
    ) -> Result<ReceiverRunReservation, ReceiverRunTabError> {
        self.session_tabs.reserve_receiver_run()
    }

    pub(crate) fn insert_reserved_receiver_run(
        &mut self,
        reservation: &ReceiverRunReservation,
        job_id: ReceiverJobId,
        title: String,
        instance: String,
        controller: AgentController,
    ) -> SessionTabId {
        self.session_tabs
            .insert_receiver_run(reservation, job_id, title, instance, controller)
    }

    pub(crate) fn remove_receiver_run(&mut self, id: SessionTabId) -> Option<RemovedReceiverRun> {
        self.session_tabs.remove_receiver_run(id)
    }

    pub(crate) fn detach_receiver_run_controller(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Option<AgentController> {
        self.session_tabs
            .detach_receiver_run_controller(id, job_id, instance)
    }

    pub(crate) fn shutdown_receiver_run(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Result<bool, AgentError> {
        self.session_tabs
            .shutdown_receiver_run(id, job_id, instance)
    }

    pub(crate) fn remove_shutdown_receiver_run(
        &mut self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
    ) -> Option<RemovedReceiverRun> {
        self.session_tabs
            .remove_shutdown_receiver_run(id, job_id, instance)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn receiver_run_controller(&self, id: SessionTabId) -> Option<&AgentController> {
        self.session_tabs.receiver_run_controller(id)
    }
}
