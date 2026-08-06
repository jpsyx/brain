use std::sync::{Arc, Mutex};

use brain::access::{AccessMode, capability_plan_for};
use brain::actor::ActorContext;
use brain::agent::{
    AgentController, AgentError, AgentFrontend, AgentSession, AgentTransport, ClaudeFrontend,
    CodexFrontend, InputSequence, LaunchRequest, LaunchSpec, SessionPlan,
};

use super::setup::{Scenario, command_context};

pub(crate) fn assert_frontend_neutral_workspace_only_launch(
    scenario: &Scenario,
    actor: &ActorContext,
) {
    let config = brain::config::Config::load(&scenario.family);
    let plan = capability_plan_for(&config, &command_context(scenario))
        .expect("selected family capability plan");
    assert_eq!(plan.credentials.source_workspace(), scenario.family.id());
    assert_eq!(plan.mcps.names(), ["family-notes"]);
    assert!(!format!("{plan:?}").contains("personal-capability-secret"));

    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&scenario.family),
        actor.clone(),
        SessionPlan::fresh(AgentSession::new("acceptance-session").expect("session")),
        Some("Create the family task".to_owned()),
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);
    let captured = Arc::new(Mutex::new(Vec::new()));

    for frontend in frontends(scenario) {
        let transport = RecordingTransport {
            launches: Arc::clone(&captured),
        };
        let mut controller = AgentController::new(
            Arc::clone(&scenario.family),
            actor.clone(),
            frontend,
            Box::new(transport),
        );
        controller.launch(&request).expect("controller launch");
    }

    let launches = captured.lock().expect("recorded launches");
    assert_eq!(launches.len(), 2);
    for launch in launches.iter() {
        assert_eq!(launch.cwd, scenario.family.root());
        assert!(launch.command.contains("Workspace root:"));
        assert!(
            launch
                .command
                .contains(&scenario.family.root().display().to_string())
        );
        assert!(launch.command.contains("advisory prompt enforcement"));
        assert!(!launch.command.contains("personal-capability-secret"));
        assert!(
            launch
                .environment
                .iter()
                .all(|(_, value)| value != "personal-capability-secret")
        );
    }
}

fn frontends(scenario: &Scenario) -> [Box<dyn AgentFrontend>; 2] {
    [
        Box::new(ClaudeFrontend::new(
            "claude",
            scenario.family.root().to_path_buf(),
            scenario.home.join(".claude/projects"),
        )),
        Box::new(CodexFrontend::new("codex")),
    ]
}

struct RecordingTransport {
    launches: Arc<Mutex<Vec<LaunchSpec>>>,
}

impl AgentTransport for RecordingTransport {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        self.launches
            .lock()
            .expect("recording transport")
            .push(spec.clone());
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
