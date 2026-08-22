use crate::actor::ActorContext;
#[cfg(test)]
use crate::agent::AgentTransport;
use crate::agent::{AgentController, AgentError};
use crate::skill_session::SkillSessionKey;
use crate::tui::model::{BrainTab, SessionTabId};

#[cfg(test)]
pub(super) mod exhausted_tab_ids;

pub(crate) struct BrainPanelStateInit {
    pub(crate) instance: String,
    pub(crate) interactive_actor: ActorContext,
    pub(crate) configured_skill_sessions: Option<serde_json::Value>,
}

struct SkillSessionTab {
    pub(crate) id: SessionTabId,
    pub(crate) key: SkillSessionKey,
    pub(crate) title: String,
    pub(crate) token: String,
    pub(crate) controller: AgentController,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SkillSessionTabIdExhausted;

impl std::fmt::Display for SkillSessionTabIdExhausted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("skill-session tab identity exhausted")
    }
}

impl std::error::Error for SkillSessionTabIdExhausted {}

pub(crate) struct BrainPanelState {
    main: Option<AgentController>,
    brain_turn_active: bool,
    skill_sessions: Vec<SkillSessionTab>,
    next_session_tab_id: u32,
    configured_skill_sessions: Option<serde_json::Value>,
    instance: String,
    interactive_actor: ActorContext,
    session_actor: Option<ActorContext>,
    #[cfg(test)]
    brain_transport_override: Option<Box<dyn AgentTransport>>,
    #[cfg(test)]
    session_done_url_override: Option<String>,
    #[cfg(test)]
    session_transport_override: Option<Box<dyn AgentTransport>>,
}

impl BrainPanelState {
    pub(crate) fn new(init: BrainPanelStateInit) -> Self {
        Self {
            main: None,
            brain_turn_active: false,
            skill_sessions: Vec::new(),
            next_session_tab_id: 0,
            configured_skill_sessions: init.configured_skill_sessions,
            instance: init.instance,
            interactive_actor: init.interactive_actor,
            session_actor: None,
            #[cfg(test)]
            brain_transport_override: None,
            #[cfg(test)]
            session_done_url_override: None,
            #[cfg(test)]
            session_transport_override: None,
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

    pub(crate) const fn clear_turn(&mut self) {
        self.brain_turn_active = false;
    }

    #[must_use]
    pub(crate) fn instance(&self) -> &str {
        &self.instance
    }

    #[must_use]
    pub(crate) const fn interactive_actor(&self) -> &ActorContext {
        &self.interactive_actor
    }

    #[must_use]
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
        self.main.is_some() || !self.skill_sessions.is_empty()
    }

    #[must_use]
    pub(crate) fn skill_session_tab_ids(&self) -> Vec<SessionTabId> {
        self.skill_sessions.iter().map(|tab| tab.id).collect()
    }

    #[must_use]
    pub(crate) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.skill_sessions.iter().map(|tab| tab.key).collect()
    }

    #[must_use]
    pub(crate) fn skill_session_rows(&self) -> Vec<(SkillSessionKey, String)> {
        self.skill_sessions
            .iter()
            .map(|tab| (tab.key, tab.title.clone()))
            .collect()
    }

    #[must_use]
    pub(crate) fn skill_session_observations(&self) -> Vec<SkillSessionObservation> {
        self.skill_sessions
            .iter()
            .map(|tab| SkillSessionObservation {
                id: tab.id,
                title: tab.title.clone(),
                token: tab.token.clone(),
                exited: tab.controller.is_alive().is_ok_and(|alive| !alive),
            })
            .collect()
    }

    #[must_use]
    pub(crate) fn skill_session_id(&self, key: SkillSessionKey) -> Option<SessionTabId> {
        self.skill_sessions
            .iter()
            .find(|tab| tab.key == key)
            .map(|tab| tab.id)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn skill_session_token(&self, key: SkillSessionKey) -> Option<String> {
        self.skill_sessions
            .iter()
            .find(|tab| tab.key == key)
            .map(|tab| tab.token.clone())
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn has_skill_session(&self, key: SkillSessionKey) -> bool {
        self.skill_sessions.iter().any(|tab| tab.key == key)
    }

    pub(crate) fn add_skill_session(
        &mut self,
        key: SkillSessionKey,
        title: String,
        token: String,
        mut controller: AgentController,
    ) -> Result<SessionTabId, SkillSessionTabIdExhausted> {
        let next_id = self.next_session_tab_id.checked_add(1).ok_or_else(|| {
            let _ = controller.shutdown();
            SkillSessionTabIdExhausted
        })?;
        let id = SessionTabId(self.next_session_tab_id);
        self.skill_sessions.push(SkillSessionTab {
            id,
            key,
            title,
            token,
            controller,
        });
        self.next_session_tab_id = next_id;
        Ok(id)
    }

    pub(crate) fn remove_skill_session(&mut self, id: SessionTabId) -> Option<RemovedSkillSession> {
        let index = self.skill_sessions.iter().position(|tab| tab.id == id)?;
        let mut tab = self.skill_sessions.remove(index);
        let _ = tab.controller.shutdown();
        Some(RemovedSkillSession { token: tab.token })
    }

    #[must_use]
    pub(crate) fn active_controller(&self, tab: BrainTab) -> Option<&AgentController> {
        match tab {
            BrainTab::Session(id) => self
                .skill_sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| &session.controller),
            BrainTab::Main => self.main.as_ref(),
        }
    }

    #[must_use]
    pub(crate) fn active_controller_mut(&mut self, tab: BrainTab) -> Option<&mut AgentController> {
        match tab {
            BrainTab::Session(id) => self
                .skill_sessions
                .iter_mut()
                .find(|session| session.id == id)
                .map(|session| &mut session.controller),
            BrainTab::Main => self.main.as_mut(),
        }
    }

    #[must_use]
    pub(crate) fn active_tab_title(&self, tab: BrainTab) -> Option<&str> {
        match tab {
            BrainTab::Session(id) => self
                .skill_sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| session.title.as_str()),
            BrainTab::Main => None,
        }
    }

    #[must_use]
    pub(crate) fn tab_titles(&self) -> Vec<String> {
        let mut titles = vec!["Brain".to_owned()];
        titles.extend(
            self.skill_sessions
                .iter()
                .map(|session| session.title.clone()),
        );
        titles
    }

    pub(crate) fn shutdown_controllers(&mut self) -> Vec<AgentError> {
        let mut errors = Vec::new();
        for controller in self.main.iter_mut().chain(
            self.skill_sessions
                .iter_mut()
                .map(|session| &mut session.controller),
        ) {
            if let Err(error) = controller.shutdown() {
                errors.push(error);
            }
        }
        errors
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
