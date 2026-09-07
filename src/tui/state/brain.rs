use crate::actor::ActorContext;
#[cfg(test)]
use crate::agent::AgentTransport;
use crate::agent::{AgentController, AgentError};
use crate::manual_session::{ManualSessionId, ManualSessionRecord};
use crate::skill_session::SkillSessionKey;
use crate::tui::model::{BrainTab, SessionTabId};

#[cfg(test)]
pub(super) mod exhausted_tab_ids;
mod receiver;
mod sessions;

use sessions::SessionTabs;
pub(crate) use sessions::{
    ManualSessionObservation, ManualSessionTabIdExhausted, ReceiverRunObservation, ReceiverRunPoll,
    ReceiverRunPollError, ReceiverRunReservation, ReceiverRunTabError, RemovedManualSession,
    RemovedReceiverRun, RemovedSkillSession, SessionPaletteEntry, SessionRenameEntry,
    SkillSessionObservation, SkillSessionTabIdExhausted,
};

pub(crate) struct BrainPanelStateInit {
    pub(crate) instance: String,
    pub(crate) manual_sessions: Vec<ManualSessionRecord>,
    pub(crate) interactive_actor: ActorContext,
    pub(crate) configured_skill_sessions: Option<serde_json::Value>,
}

pub(crate) struct BrainPanelState {
    main: Option<AgentController>,
    brain_turn_active: bool,
    session_tabs: SessionTabs,
    configured_skill_sessions: Option<serde_json::Value>,
    main_manual_session_id: ManualSessionId,
    manual_sessions_to_restore: Option<Vec<ManualSessionRecord>>,
    interactive_actor: ActorContext,
    interactive_response_id: Option<String>,
    interactive_agent_session_id: Option<String>,
    resume_refusals: crate::tui::app_brain::launch::arrival::ResumeRefusals,
    session_actor: Option<ActorContext>,
    #[cfg(test)]
    brain_transport_override: std::collections::VecDeque<Box<dyn AgentTransport>>,
    #[cfg(test)]
    manual_transport_override: std::collections::VecDeque<Box<dyn AgentTransport>>,
    #[cfg(test)]
    session_done_url_override: Option<String>,
    #[cfg(test)]
    session_transport_override: Option<Box<dyn AgentTransport>>,
    #[cfg(test)]
    receiver_transport_override: Option<Box<dyn AgentTransport>>,
}

impl BrainPanelState {
    pub(crate) fn new(init: BrainPanelStateInit) -> Self {
        let main_manual_session_id = init
            .manual_sessions
            .iter()
            .find(|record| record.role == crate::manual_session::ManualSessionRole::Main)
            .map_or_else(
                || ManualSessionId::parse(&init.instance).unwrap_or_default(),
                |record| record.id.clone(),
            );
        Self {
            main: None,
            brain_turn_active: false,
            session_tabs: SessionTabs::default(),
            configured_skill_sessions: init.configured_skill_sessions,
            main_manual_session_id,
            manual_sessions_to_restore: Some(init.manual_sessions),
            interactive_actor: init.interactive_actor,
            interactive_response_id: None,
            interactive_agent_session_id: None,
            resume_refusals: crate::tui::app_brain::launch::arrival::ResumeRefusals::default(),
            session_actor: None,
            #[cfg(test)]
            brain_transport_override: std::collections::VecDeque::new(),
            #[cfg(test)]
            manual_transport_override: std::collections::VecDeque::new(),
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
        self.main_manual_session_id.as_str()
    }

    pub(crate) const fn main_manual_session_id(&self) -> &ManualSessionId {
        &self.main_manual_session_id
    }

    pub(crate) fn take_manual_sessions_to_restore(&mut self) -> Option<Vec<ManualSessionRecord>> {
        self.manual_sessions_to_restore.take()
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

    pub(crate) fn arm_main_startup(&mut self, resumed: Option<String>, now: std::time::Instant) {
        self.resume_refusals.arm(resumed, now);
    }

    pub(crate) fn main_exit_decision(
        &mut self,
        alive: bool,
        now: std::time::Instant,
    ) -> Option<crate::tui::app_brain::launch::arrival::ExitedPanel> {
        self.resume_refusals.observe(alive, now)
    }

    pub(crate) fn disarm_resume_arrival(&mut self) {
        self.resume_refusals.disarm();
    }

    /// Never offer this id again for the rest of the run.
    pub(crate) fn refuse_resume_id(&mut self, session_id: String) {
        self.resume_refusals.refuse(session_id);
    }

    #[must_use]
    pub(crate) fn resume_was_refused(&self, session_id: &str) -> bool {
        self.resume_refusals.was_refused(session_id)
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
        !self.main_manual_session_id.as_str().is_empty()
    }

    #[must_use]
    pub(crate) fn session_tab_ids(&self) -> Vec<SessionTabId> {
        self.session_tabs.ids()
    }

    pub(crate) fn user_session_rows(&self) -> Vec<SessionPaletteEntry> {
        self.session_tabs.user_session_rows()
    }

    pub(crate) fn manual_session_rows(&self) -> Vec<SessionPaletteEntry> {
        self.session_tabs.manual_session_rows()
    }

    pub(crate) fn rename_session_rows(&self) -> Vec<SessionRenameEntry> {
        std::iter::once(SessionRenameEntry {
            id: None,
            title: crate::manual_session::MAIN_SESSION_TITLE.to_owned(),
            renameable: false,
        })
        .chain(self.session_tabs.rename_session_rows())
        .collect()
    }

    pub(crate) fn add_manual_session(
        &mut self,
        record: ManualSessionRecord,
        controller: AgentController,
        resumed_session_id: Option<String>,
    ) -> Result<SessionTabId, ManualSessionTabIdExhausted> {
        self.session_tabs
            .add_manual_session(record, controller, resumed_session_id)
    }

    pub(crate) fn remove_manual_session(
        &mut self,
        id: SessionTabId,
    ) -> Option<RemovedManualSession> {
        self.session_tabs.remove_manual_session(id)
    }

    pub(crate) fn replace_manual_controller(
        &mut self,
        id: SessionTabId,
        record: &ManualSessionRecord,
        controller: AgentController,
        resumed_session_id: Option<String>,
    ) -> anyhow::Result<()> {
        self.session_tabs
            .replace_manual_controller(id, record, controller, resumed_session_id)
    }

    pub(crate) fn manual_session_id(&self, id: SessionTabId) -> Option<&ManualSessionId> {
        self.session_tabs.manual_session_id(id)
    }

    pub(crate) fn rename_manual_session(
        &mut self,
        id: SessionTabId,
        record: &ManualSessionRecord,
    ) -> anyhow::Result<()> {
        self.session_tabs.rename_manual_session(id, record)
    }

    pub(crate) fn manual_session_observations(&self) -> Vec<ManualSessionObservation> {
        self.session_tabs.manual_session_observations()
    }

    pub(crate) fn record_manual_restore_failed(&mut self, id: SessionTabId) {
        self.session_tabs.record_manual_restore_failed(id);
    }

    pub(crate) fn arm_manual_startup(&mut self, id: SessionTabId, now: std::time::Instant) {
        self.session_tabs.arm_manual_startup(id, now);
    }

    pub(crate) fn manual_exit_decision(
        &mut self,
        observation: &ManualSessionObservation,
        now: std::time::Instant,
    ) -> Option<crate::tui::app_brain::launch::arrival::ExitedPanel> {
        self.session_tabs.manual_exit_decision(observation, now)
    }

    pub(crate) fn is_manual_session_tab(&self, tab: BrainTab) -> bool {
        matches!(tab, BrainTab::Session(id) if self.manual_session_id(id).is_some())
    }

    pub(crate) fn is_receiver_session_tab(&self, tab: BrainTab) -> bool {
        matches!(tab, BrainTab::Session(id) if self.session_tabs.is_receiver_session(id))
    }

    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn skill_session_tab_ids(&self) -> Vec<SessionTabId> {
        self.session_tabs.skill_session_ids()
    }

    #[must_use]
    pub(crate) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.session_tabs.running_skill_session_keys()
    }

    #[must_use]
    pub(crate) fn skill_session_observations(&self) -> Vec<SkillSessionObservation> {
        self.session_tabs.skill_session_observations()
    }

    #[must_use]
    pub(crate) fn skill_session_id(&self, key: SkillSessionKey) -> Option<SessionTabId> {
        self.session_tabs.skill_session_id(key)
    }

    #[must_use]
    pub(crate) fn is_skill_session_tab(&self, tab: BrainTab) -> bool {
        matches!(tab, BrainTab::Session(id) if self.session_tabs.is_skill_session(id))
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn skill_session_token(&self, key: SkillSessionKey) -> Option<String> {
        self.session_tabs.skill_session_token(key)
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
        self.session_tabs
            .add_skill_session(key, title, token, controller)
    }

    pub(crate) fn remove_skill_session(&mut self, id: SessionTabId) -> Option<RemovedSkillSession> {
        self.session_tabs.remove_skill_session(id)
    }

    #[must_use]
    pub(crate) fn active_controller(&self, tab: BrainTab) -> Option<&AgentController> {
        match tab {
            BrainTab::Session(id) => self.session_tabs.controller(id),
            BrainTab::Main => self.main.as_ref(),
        }
    }

    #[must_use]
    pub(crate) fn active_controller_mut(&mut self, tab: BrainTab) -> Option<&mut AgentController> {
        match tab {
            BrainTab::Session(id) => self.session_tabs.controller_mut(id),
            BrainTab::Main => self.main.as_mut(),
        }
    }

    #[must_use]
    pub(crate) fn active_tab_title(&self, tab: BrainTab) -> Option<&str> {
        match tab {
            BrainTab::Session(id) => self.session_tabs.title(id),
            BrainTab::Main => None,
        }
    }

    #[must_use]
    pub(crate) fn tab_titles(&self) -> Vec<String> {
        let mut titles = vec!["Brain".to_owned()];
        titles.extend(self.session_tabs.titles().map(str::to_owned));
        titles
    }

    pub(crate) fn shutdown_controllers(&mut self) -> Vec<AgentError> {
        let mut errors = Vec::new();
        if let Some(controller) = &mut self.main {
            if let Err(error) = controller.shutdown() {
                errors.push(error);
            }
        }
        errors.extend(self.session_tabs.shutdown_controllers());
        errors
    }

    #[cfg(test)]
    pub(super) const fn set_next_session_tab_id(&mut self, next_id: u32) {
        self.session_tabs.set_next_id(next_id);
    }

    #[cfg(test)]
    pub(super) const fn next_session_tab_id(&self) -> u32 {
        self.session_tabs.next_id()
    }

    /// Queue a transport for the next panel launch. Successive calls line up in
    /// order, so a test can drive a launch *and* the relaunch that follows a
    /// refused resume.
    #[cfg(test)]
    pub(crate) fn replace_brain_transport(&mut self, transport: Box<dyn AgentTransport>) {
        self.brain_transport_override.push_back(transport);
    }

    #[cfg(test)]
    pub(crate) fn take_brain_transport(&mut self) -> Option<Box<dyn AgentTransport>> {
        self.brain_transport_override.pop_front()
    }

    #[cfg(test)]
    pub(crate) fn replace_manual_transport(&mut self, transport: Box<dyn AgentTransport>) {
        self.manual_transport_override.push_back(transport);
    }

    #[cfg(test)]
    pub(crate) fn take_manual_transport(&mut self) -> Option<Box<dyn AgentTransport>> {
        self.manual_transport_override.pop_front()
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

#[cfg(test)]
mod tests;
