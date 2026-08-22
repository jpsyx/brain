use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::{AgentController, AgentError, AgentKind, AgentTransport, InputSequence};
use crate::skill_session::SkillSessionKey;
use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

use super::{BrainPanelState, BrainPanelStateInit};
use crate::tui::model::SessionTabId;

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
    assert_eq!(final_id, SessionTabId(u32::MAX - 1));
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
