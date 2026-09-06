use super::*;
use crate::agent::AgentSession;
use crate::manual_session::{ManualSessionId, ManualSessionName, ManualSessionRecord};
use crate::tui::model::BrainTab;
use crate::tui::state::brain::SessionPaletteEntry;

fn brain_state() -> BrainPanelState {
    BrainPanelState::new(BrainPanelStateInit {
        instance: "shell-under-test".to_owned(),
        manual_sessions: Vec::new(),
        interactive_actor: crate::actor::test_actor("tester"),
        configured_skill_sessions: None,
    })
}

fn additional_record(id: &str, native: &str, title: &str, position: u32) -> ManualSessionRecord {
    ManualSessionRecord::additional(
        ManualSessionId::parse(id).unwrap(),
        AgentSession::new(native).unwrap(),
        ManualSessionName::parse(title, &[]).unwrap(),
        position,
    )
}

fn receiver_job_id() -> ReceiverJobId {
    ReceiverJobId::from(uuid::Uuid::parse_str("416432be-1f80-4c14-a1cd-a67990cba013").unwrap())
}

#[test]
fn manual_skill_and_receiver_tabs_share_order_but_keep_distinct_close_authority() {
    let mut brain = brain_state();
    let manual = brain
        .add_manual_session(
            additional_record("manual-id", "native", "Atlas", 1),
            controller(AgentKind::Claude),
            Some("native".to_owned()),
        )
        .unwrap();
    let skill = brain
        .add_skill_session(
            SkillSessionKey::DailyTriage,
            "Daily triage".to_owned(),
            "token".to_owned(),
            controller(AgentKind::Codex),
        )
        .unwrap();
    let receiver = brain
        .add_receiver_run(
            receiver_job_id(),
            "Receiver · SMS".to_owned(),
            "receiver-instance".to_owned(),
            controller(AgentKind::OpenCode),
        )
        .unwrap();

    assert_eq!(brain.session_tab_ids(), [manual, skill, receiver]);
    assert_eq!(
        brain.user_session_rows(),
        [
            SessionPaletteEntry::new(manual, "Atlas"),
            SessionPaletteEntry::new(skill, "Daily triage")
        ]
    );
    assert_eq!(
        brain.manual_session_rows(),
        [SessionPaletteEntry::new(manual, "Atlas")]
    );
    assert_eq!(
        brain.tab_titles(),
        ["Brain", "Atlas", "Daily triage", "Receiver · SMS"]
    );
    assert_eq!(
        brain.manual_session_id(manual).unwrap().as_str(),
        "manual-id"
    );
    assert!(brain.manual_session_id(skill).is_none());
    for (tab, expected) in [
        (BrainTab::Main, (false, false, false)),
        (BrainTab::Session(manual), (true, false, false)),
        (BrainTab::Session(skill), (false, true, false)),
        (BrainTab::Session(receiver), (false, false, true)),
        (BrainTab::Session(SessionTabId(99)), (false, false, false)),
    ] {
        assert_eq!(
            (
                brain.is_manual_session_tab(tab),
                brain.is_skill_session_tab(tab),
                brain.is_receiver_session_tab(tab)
            ),
            expected
        );
    }
    assert!(brain.remove_manual_session(receiver).is_none());
    assert!(brain.remove_manual_session(skill).is_none());
    assert!(brain.remove_skill_session(manual).is_none());
    assert!(brain.remove_receiver_run(manual).is_none());
    assert_eq!(brain.session_tab_ids(), [manual, skill, receiver]);

    let observations = brain.manual_session_observations();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].id, manual);
    assert_eq!(observations[0].manual_session_id.as_str(), "manual-id");
    assert_eq!(
        observations[0].resumed_session_id.as_deref(),
        Some("native")
    );
    assert_eq!(observations[0].alive, Some(true));
    let removed = brain.remove_manual_session(manual).unwrap();
    assert_eq!(removed.id.as_str(), "manual-id");
    assert_eq!(brain.session_tab_ids(), [skill, receiver]);
    assert_eq!(
        brain.tab_titles(),
        ["Brain", "Daily triage", "Receiver · SMS"]
    );
    let next = brain
        .add_manual_session(
            additional_record("next", "next-native", "Orion", 2),
            controller(AgentKind::Claude),
            None,
        )
        .unwrap();
    assert_eq!(next, SessionTabId(3));
    assert_eq!(brain.session_tab_ids(), [skill, receiver, next]);
}

#[test]
fn manual_tab_exhaustion_shuts_down_rejected_controller_without_mutating_tabs() {
    let mut brain = brain_state();
    brain.set_next_session_tab_id(u32::MAX - 1);
    let final_id = brain
        .add_manual_session(
            additional_record("last", "native", "Atlas", 1),
            controller(AgentKind::Claude),
            None,
        )
        .unwrap();
    assert_eq!(final_id, SessionTabId(u32::MAX - 1));
    let shutdown = Arc::new(AtomicBool::new(false));
    let rejected = AgentController::for_workspace_with_command(
        workspace(),
        AgentKind::Codex,
        "codex".to_owned(),
        crate::actor::test_actor("tester"),
        Box::new(ShutdownRecordingTransport(Arc::clone(&shutdown))),
    );
    let error = brain
        .add_manual_session(
            additional_record("rejected", "native", "Orion", 2),
            rejected,
            None,
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "manual-session tab identity exhausted");
    assert_eq!(brain.session_tab_ids(), [final_id]);
    assert_eq!(brain.next_session_tab_id(), u32::MAX);
    assert!(shutdown.load(Ordering::SeqCst));
}

#[test]
fn permanent_main_keeps_the_panel_visible_across_manual_tab_lifecycle() {
    let mut brain = brain_state();
    assert!(brain.any_panel_visible());
    let manual = brain
        .add_manual_session(
            additional_record("manual", "native", "Atlas", 1),
            controller(AgentKind::Claude),
            None,
        )
        .unwrap();
    assert!(brain.any_panel_visible());
    brain.remove_manual_session(manual).unwrap();
    assert!(brain.any_panel_visible());
}

struct FallibleShutdownTransport {
    shutdown: Arc<AtomicBool>,
    fails: bool,
}

impl AgentTransport for FallibleShutdownTransport {
    fn spawn(&mut self, _: &crate::agent::LaunchSpec) -> Result<(), AgentError> {
        Ok(())
    }
    fn send(&mut self, _: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }
    fn snapshot(&self) -> String {
        String::new()
    }
    fn is_alive(&self) -> bool {
        true
    }
    fn shutdown(&mut self) -> Result<(), AgentError> {
        self.shutdown.store(true, Ordering::SeqCst);
        if self.fails {
            Err(AgentError::Transport("shutdown failed".to_owned()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn shutdown_attempts_main_and_every_kind_after_a_manual_shutdown_failure() {
    let mut brain = brain_state();
    let flags: Vec<_> = (0..4).map(|_| Arc::new(AtomicBool::new(false))).collect();
    let mut controllers = flags.iter().enumerate().map(|(index, flag)| {
        AgentController::for_workspace_with_command(
            workspace(),
            AgentKind::Claude,
            "claude".to_owned(),
            crate::actor::test_actor("tester"),
            Box::new(FallibleShutdownTransport {
                shutdown: Arc::clone(flag),
                fails: index == 1,
            }),
        )
    });
    brain.install_main(controllers.next().unwrap());
    brain
        .add_manual_session(
            additional_record("manual", "native", "Atlas", 1),
            controllers.next().unwrap(),
            None,
        )
        .unwrap();
    brain
        .add_skill_session(
            SkillSessionKey::DailyTriage,
            "Daily triage".to_owned(),
            "token".to_owned(),
            controllers.next().unwrap(),
        )
        .unwrap();
    brain
        .add_receiver_run(
            receiver_job_id(),
            "Receiver · SMS".to_owned(),
            "receiver-instance".to_owned(),
            controllers.next().unwrap(),
        )
        .unwrap();
    assert_eq!(brain.shutdown_controllers().len(), 1);
    assert!(flags.iter().all(|flag| flag.load(Ordering::SeqCst)));
}
