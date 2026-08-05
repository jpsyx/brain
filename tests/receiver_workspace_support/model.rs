use std::time::Instant;

use brain::server::receiver::{AttachmentRef, Channel, DispatchPipeline, InboundJob};
use brain::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

pub const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
pub const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

pub fn workspace(temp: &tempfile::TempDir, id: &str, name: &str) -> WorkspaceContext {
    let root = temp.path().join(name);
    std::fs::create_dir_all(&root).unwrap();
    WorkspaceContext::new(
        temp.path(),
        WorkspaceId::parse(id).unwrap(),
        WorkspaceName::parse(name).unwrap(),
        &root,
        "member",
        temp.path(),
    )
    .unwrap()
}

pub fn job(workspace: &WorkspaceContext, prompt: &str) -> InboundJob {
    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: workspace.id(),
        actor: actor(),
        channel: Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        prompt: prompt.to_owned(),
        attachments: vec![AttachmentRef {
            url: "https://media.example.test/photo.jpg".to_owned(),
            content_type: Some("image/jpeg".to_owned()),
            filename: Some("photo.jpg".to_owned()),
        }],
        received_at_unix_ms: 1_786_000_000_000,
        provider_id: None,
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
    }
}

fn actor() -> brain::actor::ActorContext {
    let users = brain::users::Users {
        schema_version: brain::users::USERS_SCHEMA_VERSION,
        users: vec![brain::users::User {
            id: brain::users::UserId::parse("member").unwrap(),
            name: "Member".to_owned(),
            phones: vec![brain::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    brain::actor::resolve_actor(
        &brain::users::UserId::parse("member").unwrap(),
        brain::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .unwrap()
}

pub fn poll_until(deadline: Instant, mut condition: impl FnMut() -> bool) {
    while !condition() {
        assert!(Instant::now() < deadline, "condition was not reached");
        std::thread::yield_now();
    }
}

pub struct RecordingPipeline {
    workspace: &'static str,
    actor: &'static str,
    pub events: Vec<&'static str>,
}

impl RecordingPipeline {
    pub fn new(workspace: &'static str, actor: &'static str) -> Self {
        Self {
            workspace,
            actor,
            events: Vec::new(),
        }
    }
}

impl DispatchPipeline for RecordingPipeline {
    type Workspace = &'static str;
    type ProviderConfig = &'static str;
    type Authenticated = &'static str;
    type Actor = &'static str;
    type Job = &'static str;

    fn resolve_workspace(&mut self) -> anyhow::Result<Self::Workspace> {
        self.events.push("resolve");
        Ok(self.workspace)
    }

    fn load_provider_config(
        &mut self,
        workspace: &Self::Workspace,
    ) -> anyhow::Result<Self::ProviderConfig> {
        assert_eq!(*workspace, self.workspace);
        self.events.push("credentials");
        Ok("workspace-token")
    }

    fn verify_signature(
        &mut self,
        _config: &Self::ProviderConfig,
    ) -> anyhow::Result<Self::Authenticated> {
        self.events.push("signature");
        Ok("+12125550100")
    }

    fn resolve_actor(
        &mut self,
        workspace: &Self::Workspace,
        _authenticated: &Self::Authenticated,
    ) -> anyhow::Result<Self::Actor> {
        assert_eq!(*workspace, self.workspace);
        self.events.push("actor");
        Ok(self.actor)
    }

    fn build_job(
        &mut self,
        _workspace: &Self::Workspace,
        actor: &Self::Actor,
        _authenticated: &Self::Authenticated,
    ) -> anyhow::Result<Self::Job> {
        self.events.push("job");
        Ok(*actor)
    }

    fn forward(&mut self, workspace: &Self::Workspace, _job: &Self::Job) -> anyhow::Result<()> {
        assert_eq!(*workspace, self.workspace);
        self.events.push("forward");
        Ok(())
    }
}
