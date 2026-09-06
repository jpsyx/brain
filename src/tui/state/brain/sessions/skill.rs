use crate::agent::AgentController;
use crate::skill_session::SkillSessionKey;
use crate::tui::model::SessionTabId;

use super::{
    RemovedSkillSession, SessionTabIdExhausted, SessionTabKind, SessionTabs, SkillSessionMetadata,
    SkillSessionObservation, SkillSessionTabIdExhausted,
};

impl SessionTabs {
    pub(in crate::tui::state::brain) fn skill_session_ids(&self) -> Vec<SessionTabId> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                SessionTabKind::Skill(_) => Some(tab.id),
                SessionTabKind::Manual(_) | SessionTabKind::Receiver(_) => None,
            })
            .collect()
    }

    pub(in crate::tui::state::brain) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                SessionTabKind::Skill(session) => Some(session.key),
                SessionTabKind::Manual(_) | SessionTabKind::Receiver(_) => None,
            })
            .collect()
    }

    pub(in crate::tui::state::brain) fn skill_session_observations(
        &self,
    ) -> Vec<SkillSessionObservation> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                SessionTabKind::Skill(session) => Some(SkillSessionObservation {
                    id: tab.id,
                    title: tab.title.clone(),
                    token: session.token.clone(),
                    exited: tab.controller.is_alive().is_ok_and(|alive| !alive),
                }),
                SessionTabKind::Manual(_) | SessionTabKind::Receiver(_) => None,
            })
            .collect()
    }

    pub(in crate::tui::state::brain) fn skill_session_id(
        &self,
        key: SkillSessionKey,
    ) -> Option<SessionTabId> {
        self.tabs
            .iter()
            .find(|tab| {
                matches!(
                    &tab.metadata,
                    SessionTabKind::Skill(session) if session.key == key
                )
            })
            .map(|tab| tab.id)
    }

    pub(in crate::tui::state::brain) fn is_skill_session(&self, id: SessionTabId) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.id == id && matches!(&tab.metadata, SessionTabKind::Skill(_)))
    }

    #[cfg(test)]
    pub(in crate::tui::state::brain) fn skill_session_token(
        &self,
        key: SkillSessionKey,
    ) -> Option<String> {
        self.tabs.iter().find_map(|tab| match &tab.metadata {
            SessionTabKind::Skill(session) if session.key == key => Some(session.token.clone()),
            SessionTabKind::Skill(_) | SessionTabKind::Manual(_) | SessionTabKind::Receiver(_) => {
                None
            }
        })
    }

    pub(in crate::tui::state::brain) fn add_skill_session(
        &mut self,
        key: SkillSessionKey,
        title: String,
        token: String,
        controller: AgentController,
    ) -> Result<SessionTabId, SkillSessionTabIdExhausted> {
        self.add(
            title,
            SessionTabKind::Skill(SkillSessionMetadata { key, token }),
            controller,
        )
        .map_err(|SessionTabIdExhausted| SkillSessionTabIdExhausted)
    }

    pub(in crate::tui::state::brain) fn remove_skill_session(
        &mut self,
        id: SessionTabId,
    ) -> Option<RemovedSkillSession> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id && matches!(&tab.metadata, SessionTabKind::Skill(_)))?;
        let mut tab = self.tabs.remove(index);
        let _ = tab.controller.shutdown();
        let SessionTabKind::Skill(session) = tab.metadata else {
            unreachable!("the located tab was a skill session")
        };
        Some(RemovedSkillSession {
            token: session.token,
        })
    }
}
