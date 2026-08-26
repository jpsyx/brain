use crate::actor::ActorContext;
#[cfg(test)]
use crate::agent::AgentTransport;
use crate::agent::{AgentController, AgentError};
use crate::skill_session::SkillSessionKey;
use crate::state::ReceiverJobId;
use crate::tui::model::{BrainTab, SessionTabId};

mod ephemeral;
#[cfg(test)]
pub(super) mod exhausted_tab_ids;

use ephemeral::EphemeralTabs;
pub(crate) use ephemeral::{
    ReceiverRunObservation, ReceiverRunPoll, ReceiverRunPollError, ReceiverRunTabError,
    RemovedReceiverRun, RemovedSkillSession, SkillSessionObservation, SkillSessionTabIdExhausted,
};

pub(crate) struct BrainPanelStateInit {
    pub(crate) instance: String,
    pub(crate) interactive_actor: ActorContext,
    pub(crate) configured_skill_sessions: Option<serde_json::Value>,
}

pub(crate) struct BrainPanelState {
    main: Option<AgentController>,
    brain_turn_active: bool,
    ephemeral_tabs: EphemeralTabs,
    configured_skill_sessions: Option<serde_json::Value>,
    instance: String,
    interactive_actor: ActorContext,
    interactive_response_id: Option<String>,
    interactive_agent_session_id: Option<String>,
    session_actor: Option<ActorContext>,
    #[cfg(test)]
    brain_transport_override: Option<Box<dyn AgentTransport>>,
    #[cfg(test)]
    session_done_url_override: Option<String>,
    #[cfg(test)]
    session_transport_override: Option<Box<dyn AgentTransport>>,
    #[cfg(test)]
    receiver_transport_override: Option<Box<dyn AgentTransport>>,
}

impl BrainPanelState {
    pub(crate) fn new(init: BrainPanelStateInit) -> Self {
        Self {
            main: None,
            brain_turn_active: false,
            ephemeral_tabs: EphemeralTabs::default(),
            configured_skill_sessions: init.configured_skill_sessions,
            instance: init.instance,
            interactive_actor: init.interactive_actor,
            interactive_response_id: None,
            interactive_agent_session_id: None,
            session_actor: None,
            #[cfg(test)]
            brain_transport_override: None,
            #[cfg(test)]
            session_done_url_override: None,
            #[cfg(test)]
            session_transport_override: None,
            #[cfg(test)]
            receiver_transport_override: None,
        }
    }

    #[must_use]
    pub(crate) fn main_controller(&self) -> Option<&AgentController> {
        self.main.as_ref()
    }

    #[must_use]
    pub(crate) fn main_controller_mut(&mut self) -> Option<&mut AgentController> {
        self.main.as_mut()
    }

    pub(crate) fn install_main(&mut self, controller: AgentController) {
        self.session_actor = Some(controller.actor().clone());
        self.main = Some(controller);
        self.brain_turn_active = false;
    }

    pub(crate) fn take_main(&mut self) -> Option<AgentController> {
        self.session_actor = None;
        self.brain_turn_active = false;
        self.main.take()
    }

    #[must_use]
    pub(crate) const fn turn_active(&self) -> bool {
        self.brain_turn_active
    }

    pub(crate) const fn mark_turn_started(&mut self) {
        self.brain_turn_active = true;
    }

    #[must_use]
    pub(crate) fn instance(&self) -> &str {
        &self.instance
    }

    #[must_use]
    pub(crate) const fn interactive_actor(&self) -> &ActorContext {
        &self.interactive_actor
    }

    pub(crate) fn begin_interactive_session_launch(&mut self) {
        self.interactive_response_id = None;
        self.interactive_agent_session_id = None;
    }

    pub(crate) fn record_interactive_session_started(
        &mut self,
        response_id: String,
        agent_session_id: String,
    ) {
        self.interactive_response_id = Some(response_id);
        self.interactive_agent_session_id = Some(agent_session_id);
    }

    pub(crate) fn record_interactive_session_launch_failed(&mut self) {
        self.interactive_response_id = None;
        self.interactive_agent_session_id = None;
    }

    #[must_use]
    pub(crate) fn main_completion_to_clear(&self) -> Option<&str> {
        self.interactive_response_id.as_deref()
    }

    pub(crate) fn record_interactive_agent_session(&mut self, session_id: String) {
        self.interactive_agent_session_id = Some(session_id);
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn interactive_response_id(&self) -> Option<&str> {
        self.interactive_response_id.as_deref()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn interactive_agent_session_id(&self) -> Option<&str> {
        self.interactive_agent_session_id.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn record_interactive_session(
        &mut self,
        response_id: String,
        agent_session_id: String,
    ) {
        self.record_interactive_session_started(response_id, agent_session_id);
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn session_actor(&self) -> Option<&ActorContext> {
        self.session_actor.as_ref()
    }

    pub(crate) fn clear_session(&mut self) {
        self.session_actor = None;
        self.brain_turn_active = false;
    }

    #[must_use]
    pub(crate) fn configured_skill_sessions(&self) -> Option<&serde_json::Value> {
        self.configured_skill_sessions.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_configured_skill_sessions(&mut self, configured: serde_json::Value) {
        self.configured_skill_sessions = Some(configured);
    }

    #[must_use]
    pub(crate) fn any_panel_visible(&self) -> bool {
        self.main.is_some() || self.ephemeral_tabs.has_skill_sessions()
    }

    #[must_use]
    pub(crate) fn ephemeral_tab_ids(&self) -> Vec<SessionTabId> {
        self.ephemeral_tabs.ids()
    }

    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn skill_session_tab_ids(&self) -> Vec<SessionTabId> {
        self.ephemeral_tabs.skill_session_ids()
    }

    #[must_use]
    pub(crate) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.ephemeral_tabs.running_skill_session_keys()
    }

    #[must_use]
    pub(crate) fn skill_session_rows(&self) -> Vec<(SkillSessionKey, String)> {
        self.ephemeral_tabs.skill_session_rows()
    }

    #[must_use]
    pub(crate) fn skill_session_observations(&self) -> Vec<SkillSessionObservation> {
        self.ephemeral_tabs.skill_session_observations()
    }

    #[must_use]
    pub(crate) fn skill_session_id(&self, key: SkillSessionKey) -> Option<SessionTabId> {
        self.ephemeral_tabs.skill_session_id(key)
    }

    #[must_use]
    pub(crate) fn is_skill_session_tab(&self, tab: BrainTab) -> bool {
        matches!(tab, BrainTab::Session(id) if self.ephemeral_tabs.is_skill_session(id))
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn skill_session_token(&self, key: SkillSessionKey) -> Option<String> {
        self.ephemeral_tabs.skill_session_token(key)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn has_skill_session(&self, key: SkillSessionKey) -> bool {
        self.skill_session_id(key).is_some()
    }

    pub(crate) fn add_skill_session(
        &mut self,
        key: SkillSessionKey,
        title: String,
        token: String,
        controller: AgentController,
    ) -> Result<SessionTabId, SkillSessionTabIdExhausted> {
        self.ephemeral_tabs
            .add_skill_session(key, title, token, controller)
    }

    pub(crate) fn remove_skill_session(&mut self, id: SessionTabId) -> Option<RemovedSkillSession> {
        self.ephemeral_tabs.remove_skill_session(id)
    }

    #[must_use]
    pub(crate) fn active_controller(&self, tab: BrainTab) -> Option<&AgentController> {
        match tab {
            BrainTab::Session(id) => self.ephemeral_tabs.controller(id),
            BrainTab::Main => self.main.as_ref(),
        }
    }

    #[must_use]
    pub(crate) fn active_controller_mut(&mut self, tab: BrainTab) -> Option<&mut AgentController> {
        match tab {
            BrainTab::Session(id) => self.ephemeral_tabs.controller_mut(id),
            BrainTab::Main => self.main.as_mut(),
        }
    }

    #[must_use]
    pub(crate) fn active_tab_title(&self, tab: BrainTab) -> Option<&str> {
        match tab {
            BrainTab::Session(id) => self.ephemeral_tabs.title(id),
            BrainTab::Main => None,
        }
    }

    #[must_use]
    pub(crate) fn tab_titles(&self) -> Vec<String> {
        let mut titles = vec!["Brain".to_owned()];
        titles.extend(self.ephemeral_tabs.titles().map(str::to_owned));
        titles
    }

    pub(crate) fn shutdown_controllers(&mut self) -> Vec<AgentError> {
        let mut errors = Vec::new();
        if let Some(controller) = &mut self.main {
            if let Err(error) = controller.shutdown() {
                errors.push(error);
            }
        }
        errors.extend(self.ephemeral_tabs.shutdown_controllers());
        errors
    }

    #[cfg(test)]
    pub(super) const fn set_next_session_tab_id(&mut self, next_id: u32) {
        self.ephemeral_tabs.set_next_id(next_id);
    }

    #[cfg(test)]
    pub(super) const fn next_session_tab_id(&self) -> u32 {
        self.ephemeral_tabs.next_id()
    }

    #[cfg(test)]
    pub(crate) fn replace_brain_transport(&mut self, transport: Box<dyn AgentTransport>) {
        self.brain_transport_override = Some(transport);
    }

    #[cfg(test)]
    pub(crate) fn take_brain_transport(&mut self) -> Option<Box<dyn AgentTransport>> {
        self.brain_transport_override.take()
    }

    #[cfg(test)]
    pub(crate) fn replace_session_transport(&mut self, transport: Box<dyn AgentTransport>) {
        self.session_transport_override = Some(transport);
    }

    #[cfg(test)]
    pub(crate) fn take_session_transport(&mut self) -> Option<Box<dyn AgentTransport>> {
        self.session_transport_override.take()
    }

    #[cfg(test)]
    pub(crate) fn replace_receiver_transport(&mut self, transport: Box<dyn AgentTransport>) {
        self.receiver_transport_override = Some(transport);
    }

    #[cfg(test)]
    pub(crate) fn take_receiver_transport(&mut self) -> Option<Box<dyn AgentTransport>> {
        self.receiver_transport_override.take()
    }

    #[cfg(test)]
    pub(crate) fn replace_session_done_url(&mut self, url: String) {
        self.session_done_url_override = Some(url);
    }

    #[cfg(test)]
    pub(crate) fn take_session_done_url(&mut self) -> Option<String> {
        self.session_done_url_override.take()
    }
}

impl BrainPanelState {
    #[must_use]
    pub(crate) fn receiver_run_observations(&self) -> Vec<ReceiverRunObservation> {
        self.ephemeral_tabs.receiver_run_observations()
    }

    pub(crate) fn poll_receiver_run(
        &self,
        id: SessionTabId,
        job_id: ReceiverJobId,
        instance: &str,
        request: &crate::agent::AgentObservationRequest,
    ) -> Result<ReceiverRunPoll, ReceiverRunPollError> {
        self.ephemeral_tabs
            .poll_receiver_run(id, job_id, instance, request)
    }

    pub(crate) fn add_receiver_run(
        &mut self,
        job_id: ReceiverJobId,
        title: String,
        instance: String,
        controller: AgentController,
    ) -> Result<SessionTabId, ReceiverRunTabError> {
        self.ephemeral_tabs
            .add_receiver_run(job_id, title, instance, controller)
    }

    pub(crate) fn remove_receiver_run(&mut self, id: SessionTabId) -> Option<RemovedReceiverRun> {
        self.ephemeral_tabs.remove_receiver_run(id)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn receiver_run_controller(&self, id: SessionTabId) -> Option<&AgentController> {
        self.ephemeral_tabs.receiver_run_controller(id)
    }
}

#[cfg(test)]
mod tests;
