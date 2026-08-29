use std::sync::{Arc, Mutex};

use brain::actor::{RequestIdentity, resolve_actor};
use brain::agent::{
    AgentController, AgentError, AgentKind, AgentTransport, InputSequence, LaunchRequest,
    LaunchSpec,
};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users};
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

pub(crate) fn family_id() -> WorkspaceId {
    WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace id")
}

pub(crate) fn temporary_workspace() -> (tempfile::TempDir, Arc<WorkspaceContext>) {
    let home = tempfile::tempdir().expect("temporary home");
    let root = home.path().join("family");
    std::fs::create_dir_all(root.join(".config")).expect("workspace config directory");
    let workspace = WorkspaceContext::new(
        home.path(),
        family_id(),
        WorkspaceName::parse("family").expect("workspace name"),
        &root,
        "pablo",
        home.path(),
    )
    .expect("workspace context");
    (home, Arc::new(workspace))
}

pub(crate) fn actor() -> brain::actor::ActorContext {
    named_actor("pablo", "Pablo")
}

pub(crate) fn named_actor(id: &str, name: &str) -> brain::actor::ActorContext {
    let id = UserId::parse(id).expect("user id");
    resolve_actor(
        &id,
        RequestIdentity::Local,
        &Users {
            schema_version: USERS_SCHEMA_VERSION,
            users: vec![User {
                id: id.clone(),
                name: name.to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        },
    )
    .expect("actor")
}

pub(crate) fn launch_spec(
    kind: AgentKind,
    command: &str,
    request: &LaunchRequest,
) -> Result<LaunchSpec, AgentError> {
    let captured = Arc::new(Mutex::new(None));
    let transport = RecordingTransport {
        launch: Arc::clone(&captured),
    };
    let mut controller = AgentController::for_workspace_with_command(
        Arc::clone(request.workspace()),
        kind,
        hermetic_frontend_command(kind, command),
        request.actor().clone(),
        Box::new(transport),
    );
    controller.launch(request)?;
    captured
        .lock()
        .expect("recorded launch")
        .take()
        .ok_or_else(|| AgentError::Transport("launch was not recorded".to_owned()))
}

pub(crate) fn launch_with_spawn_failure(
    kind: AgentKind,
    command: &str,
    request: &LaunchRequest,
) -> Result<(), AgentError> {
    let mut controller = AgentController::for_workspace_with_command(
        Arc::clone(request.workspace()),
        kind,
        hermetic_frontend_command(kind, command),
        request.actor().clone(),
        Box::new(FailingTransport),
    );
    controller.launch(request)
}

fn hermetic_frontend_command(kind: AgentKind, command: &str) -> String {
    if kind == AgentKind::Claude {
        let fake_claude = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude/claude")
            .display()
            .to_string();
        return command.replacen("claude", &fake_claude, 1);
    }
    if kind == AgentKind::OpenCode && command == "opencode" {
        return std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/opencode/fake_opencode.sh")
            .display()
            .to_string();
    }
    command.to_owned()
}

struct RecordingTransport {
    launch: Arc<Mutex<Option<LaunchSpec>>>,
}

impl AgentTransport for RecordingTransport {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        *self.launch.lock().expect("recording transport") = Some(spec.clone());
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

    fn shutdown(&mut self) -> Result<(), AgentError> {
        Ok(())
    }
}

struct FailingTransport;

impl AgentTransport for FailingTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        Err(AgentError::Transport("injected spawn failure".to_owned()))
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        false
    }

    fn shutdown(&mut self) -> Result<(), AgentError> {
        Ok(())
    }
}
