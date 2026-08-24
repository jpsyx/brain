use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Instant;

use brain::server::receiver::{AttachmentRef, Channel, DispatchPipeline, InboundJob};
use brain::workspace::WorkspaceContext;

pub const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
pub const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";

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
            provider_id: None,
            content_type: Some("image/jpeg".to_owned()),
            filename: Some("photo.jpg".to_owned()),
        }],
        received_at_unix_ms: 1_786_000_000_000,
        provider_id: None,
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

pub fn durable_jobs(workspace: &WorkspaceContext) -> Vec<InboundJob> {
    let path = workspace.paths().state_db();
    if !path.exists() {
        return Vec::new();
    }
    let connection = rusqlite::Connection::open(path).expect("open durable receiver state");
    let mut statement = match connection
        .prepare("SELECT inbound_json FROM receiver_jobs ORDER BY received_at_unix_ms, job_id")
    {
        Ok(statement) => statement,
        Err(error) if error.to_string().contains("no such table: receiver_jobs") => {
            return Vec::new();
        }
        Err(error) => panic!("prepare durable receiver jobs: {error}"),
    };
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query durable receiver jobs")
        .map(|row| {
            serde_json::from_str(&row.expect("durable receiver JSON"))
                .expect("parse durable receiver job")
        })
        .collect()
}

pub fn durable_conversation_count(workspace: &WorkspaceContext) -> i64 {
    let connection = rusqlite::Connection::open(workspace.paths().state_db())
        .expect("open durable receiver state");
    connection
        .query_row("SELECT COUNT(*) FROM receiver_conversations", [], |row| {
            row.get(0)
        })
        .expect("count durable receiver conversations")
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

    fn revalidate_authority(
        &mut self,
        workspace: &Self::Workspace,
        _job: &Self::Job,
    ) -> anyhow::Result<()> {
        assert_eq!(*workspace, self.workspace);
        self.events.push("authority");
        Ok(())
    }
}

pub struct RevocationPipeline {
    pub actor_resolved: Arc<Barrier>,
    pub release: Arc<Barrier>,
    pub authority_valid: Arc<AtomicBool>,
    pub forwards: Arc<AtomicUsize>,
}

impl DispatchPipeline for RevocationPipeline {
    type Workspace = ();
    type ProviderConfig = ();
    type Authenticated = ();
    type Actor = ();
    type Job = ();

    fn resolve_workspace(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn load_provider_config(&mut self, _workspace: &()) -> anyhow::Result<()> {
        Ok(())
    }

    fn verify_signature(&mut self, _config: &()) -> anyhow::Result<()> {
        Ok(())
    }

    fn resolve_actor(&mut self, _workspace: &(), _authenticated: &()) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_job(
        &mut self,
        _workspace: &(),
        _actor: &(),
        _authenticated: &(),
    ) -> anyhow::Result<()> {
        self.actor_resolved.wait();
        self.release.wait();
        Ok(())
    }

    fn revalidate_authority(&mut self, _workspace: &(), _job: &()) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.authority_valid.load(Ordering::Acquire),
            "route authority was revoked"
        );
        Ok(())
    }

    fn forward(&mut self, _workspace: &(), _job: &()) -> anyhow::Result<()> {
        self.forwards.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}
