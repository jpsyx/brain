use std::sync::{Arc, Mutex};

use brain::access::{AccessMode, capability_plan_for};
use brain::actor::ActorContext;
use brain::agent::{
    AgentController, AgentError, AgentKind, AgentSession, AgentTransport, InputSequence,
    LaunchRequest, LaunchSpec, SessionPlan,
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

    let context = command_context(scenario);
    for kind in AgentKind::ALL {
        let transport = RecordingTransport {
            launches: Arc::clone(&captured),
        };
        let mut controller =
            AgentController::configured(&context, kind, actor.clone(), Box::new(transport));
        controller.launch(&request).expect("controller launch");
    }

    let launches = captured.lock().expect("recorded launches");
    assert_eq!(launches.len(), AgentKind::ALL.len());
    for launch in launches.iter() {
        assert_eq!(launch.cwd, scenario.family.root());
        let trusted_launch_surface = std::iter::once(launch.command.as_str())
            .chain(launch.environment.iter().map(|(_, value)| value.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(trusted_launch_surface.contains("Workspace root:"));
        assert!(trusted_launch_surface.contains(&scenario.family.root().display().to_string()));
        assert!(trusted_launch_surface.contains("advisory prompt enforcement"));
        assert!(!trusted_launch_surface.contains("personal-capability-secret"));
        assert!(
            launch
                .environment
                .iter()
                .all(|(_, value)| value != "personal-capability-secret")
        );
    }
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
