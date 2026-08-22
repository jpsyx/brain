use crate::actor::ActorContext;
#[cfg(test)]
use crate::agent::AgentTransport;
use crate::agent::{AgentController, AgentError};
use crate::skill_session::SkillSessionKey;
use crate::tui::{BrainTab, SessionTabId};

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
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::agent::{AgentController, AgentError, AgentKind, AgentTransport, InputSequence};
    use crate::skill_session::SkillSessionKey;
    use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

    use super::{BrainPanelState, BrainPanelStateInit};

    struct DormantTransport;

    impl AgentTransport for DormantTransport {
        fn spawn(&mut self, _spec: &crate::agent::LaunchSpec) -> Result<(), AgentError> {
            Ok(())
        }

        fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
            Ok(())
        }

        fn snapshot(&self) -> String {
            String::new()
        }

        fn is_alive(&self) -> bool {
            true
        }

        fn shutdown(&mut self) {}
    }

    struct ShutdownRecordingTransport(Arc<AtomicBool>);

    impl AgentTransport for ShutdownRecordingTransport {
        fn spawn(&mut self, _spec: &crate::agent::LaunchSpec) -> Result<(), AgentError> {
            Ok(())
        }

        fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
            Ok(())
        }

        fn snapshot(&self) -> String {
            String::new()
        }

        fn is_alive(&self) -> bool {
            true
        }

        fn shutdown(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn workspace() -> Arc<WorkspaceContext> {
        Arc::new(
            WorkspaceContext::new(
                Path::new("/home/tester"),
                WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace id"),
                WorkspaceName::parse("family").expect("workspace name"),
                Path::new("/workspaces/family"),
                "tester",
                Path::new("/workspaces"),
            )
            .expect("workspace context"),
        )
    }

    fn controller(kind: AgentKind) -> AgentController {
        controller_for_actor(kind, crate::actor::test_actor("tester"))
    }

    fn controller_for_actor(kind: AgentKind, actor: crate::actor::ActorContext) -> AgentController {
        AgentController::for_workspace_with_command(
            workspace(),
            kind,
            kind.as_str().to_owned(),
            actor,
            Box::new(DormantTransport),
        )
    }

    #[test]
    fn brain_state_owns_main_controller_actor_and_turn_lifecycle() {
        let actor = crate::actor::test_actor("tester");
        let mut brain = BrainPanelState::new(BrainPanelStateInit {
            instance: "shell-under-test".to_owned(),
            interactive_actor: actor,
            configured_skill_sessions: None,
        });

        let controller = controller_for_actor(
            AgentKind::Codex,
            crate::actor::test_actor("remote-controller"),
        );
        let controller_actor = controller.actor().clone();
        brain.install_main(controller);
        brain.mark_turn_started();

        assert_eq!(
            brain.main_controller().map(AgentController::kind),
            Some(AgentKind::Codex)
        );
        assert_eq!(brain.session_actor(), Some(&controller_actor));
        assert_eq!(
            brain.session_actor(),
            brain.main_controller().map(AgentController::actor),
            "session completion identity must be derived from the installed controller"
        );
        assert!(brain.turn_active());
        assert_eq!(brain.instance(), "shell-under-test");

        let controller = brain.take_main().expect("owned main controller");
        assert_eq!(controller.kind(), AgentKind::Codex);
        assert!(brain.main_controller().is_none());
        assert!(brain.session_actor().is_none());
        assert!(!brain.turn_active());
    }

    #[test]
    fn brain_state_assigns_monotonic_skill_tab_ids_and_keeps_session_identity() {
        let mut brain = BrainPanelState::new(BrainPanelStateInit {
            instance: "shell-under-test".to_owned(),
            interactive_actor: crate::actor::test_actor("tester"),
            configured_skill_sessions: None,
        });

        let first = brain
            .add_skill_session(
                SkillSessionKey::DailyTriage,
                "Daily triage".to_owned(),
                "token-one".to_owned(),
                controller(AgentKind::Claude),
            )
            .expect("first tab identity");
        let removed = brain.remove_skill_session(first).expect("first tab");
        let second = brain
            .add_skill_session(
                SkillSessionKey::Custom(0),
                "Inbox".to_owned(),
                "token-two".to_owned(),
                controller(AgentKind::OpenCode),
            )
            .expect("second tab identity");

        assert_ne!(first, second, "a closed tab id must never be reused");
        assert_eq!(removed.token, "token-one");
        assert_eq!(brain.skill_session_tab_ids(), [second]);
        assert_eq!(
            brain.running_skill_session_keys(),
            [SkillSessionKey::Custom(0)]
        );
    }

    #[test]
    fn skill_tab_id_exhaustion_is_fallible_and_does_not_mutate_state() {
        let mut brain = BrainPanelState::new(BrainPanelStateInit {
            instance: "shell-under-test".to_owned(),
            interactive_actor: crate::actor::test_actor("tester"),
            configured_skill_sessions: None,
        });
        brain.next_session_tab_id = u32::MAX - 1;
        let final_id = brain
            .add_skill_session(
                SkillSessionKey::Custom(0),
                "Final identity".to_owned(),
                "token-final".to_owned(),
                controller(AgentKind::OpenCode),
            )
            .expect("the final representable allocation");
        assert_eq!(final_id, crate::tui::SessionTabId(u32::MAX - 1));
        brain
            .remove_skill_session(final_id)
            .expect("remove final representable tab");
        assert_eq!(brain.next_session_tab_id, u32::MAX);

        let shutdown = Arc::new(AtomicBool::new(false));
        let controller = AgentController::for_workspace_with_command(
            workspace(),
            AgentKind::Claude,
            AgentKind::Claude.as_str().to_owned(),
            crate::actor::test_actor("tester"),
            Box::new(ShutdownRecordingTransport(Arc::clone(&shutdown))),
        );

        let error = brain
            .add_skill_session(
                SkillSessionKey::DailyTriage,
                "Daily triage".to_owned(),
                "token-one".to_owned(),
                controller,
            )
            .expect_err("an exhausted identity space must reject the tab");

        assert_eq!(error.to_string(), "skill-session tab identity exhausted");
        assert!(brain.skill_session_tab_ids().is_empty());
        assert_eq!(brain.next_session_tab_id, u32::MAX);
        assert!(
            shutdown.load(Ordering::SeqCst),
            "a launched controller rejected by tab allocation must be shut down"
        );
    }
}
