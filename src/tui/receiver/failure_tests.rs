use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::agent::{
    AgentAction, AgentController, AgentError, AgentFrontend, AgentKind, AgentSession,
    AgentTransport, CompletionStrategy, HookMetadata, InputSequence, LaunchRequest, LaunchSpec,
    SessionScope, SessionStore,
};
use crate::server::receiver::{Channel, InboundJob};
use crate::state::{
    Db, ReceiverConversationIdentity, ReceiverJobState, ReceiverLaunchFailure,
    ReceiverLaunchRetryOutcome,
};
use crate::sync::args::Direction;
use crate::tui::app_sync::ReceiverSyncRuntime;
use crate::tui::shell::ShellRunner;
use crate::tui::state::{AppServices, AppServicesInit};
use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

use super::{ReceiverRemoteSession, ReceiverSessionRegistration, rollback_receiver_launch};

#[derive(Default)]
struct NoopRunner;

impl ShellRunner for NoopRunner {
    fn run(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn open(&self, _url: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NoopSyncRuntime;

impl ReceiverSyncRuntime for NoopSyncRuntime {
    fn monotonic_now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }

    fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::UNIX_EPOCH
    }

    fn live_sync_state(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState> {
        None
    }

    fn latest_successful_downstream_id(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<i64> {
        None
    }

    fn latest_downstream_completion(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        None
    }

    fn spawn_detached_sync(
        &self,
        _workspace: &WorkspaceContext,
        _direction: Direction,
    ) -> Option<u32> {
        None
    }
}

struct LaunchFrontend;

impl AgentFrontend for LaunchFrontend {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn launch_spec(&self, request: &LaunchRequest) -> Result<LaunchSpec, AgentError> {
        Ok(LaunchSpec::new(
            "receiver-test-agent",
            request.workspace().root().to_path_buf(),
            Vec::new(),
            HookMetadata::none(),
        ))
    }

    fn input_for(&self, _action: AgentAction<'_>) -> Result<InputSequence, AgentError> {
        Ok(InputSequence::bytes([]))
    }

    fn completion_strategy(&self) -> Result<CompletionStrategy, AgentError> {
        Ok(CompletionStrategy::Hook)
    }

    fn resume_candidate_exists(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(true)
    }

    fn response_id(&self, session: &AgentSession) -> Result<String, AgentError> {
        Ok(session.as_str().to_owned())
    }

    fn can_resume_response_session(&self, _session: &AgentSession) -> Result<bool, AgentError> {
        Ok(false)
    }
}

struct ShutdownTransport(Arc<Mutex<u32>>);

impl AgentTransport for ShutdownTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        Ok(())
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

    fn shutdown(&mut self) {
        *self.0.lock().expect("shutdown count") += 1;
    }
}

fn workspace() -> Arc<WorkspaceContext> {
    Arc::new(
        WorkspaceContext::new(
            Path::new("/home/tester"),
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace ID"),
            WorkspaceName::parse("family").expect("workspace name"),
            Path::new("/workspaces/family"),
            "test-user",
            Path::new("/home/tester"),
        )
        .expect("workspace context"),
    )
}

fn actor() -> crate::actor::ActorContext {
    let user_id = crate::users::UserId::parse("test-user").expect("user ID");
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: user_id.clone(),
            name: "Test user".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    crate::actor::resolve_actor(
        &user_id,
        crate::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .expect("receiver actor")
}

fn inbound(workspace: &WorkspaceContext, actor: &crate::actor::ActorContext) -> InboundJob {
    InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: workspace.id(),
        actor: actor.clone(),
        channel: Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        prompt: "private receiver prompt".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 100,
        provider_id: None,
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    }
}

fn services(db: Db) -> AppServices {
    AppServices::new(AppServicesInit {
        agenda_runner: Box::new(NoopRunner),
        open_runner: Box::new(NoopRunner),
        db,
        receiver_intent_refresher: Box::new(crate::server::control::ServerClient::default()),
        receiver_sync_runtime: Box::new(NoopSyncRuntime),
    })
}

#[test]
fn every_pre_acceptance_launch_failure_stops_the_controller_releases_only_remote_ownership_and_retries()
 {
    for failure in ReceiverLaunchFailure::ALL {
        let workspace = workspace();
        let actor = actor();
        let db = Db::open_in_memory().expect("state DB");
        let inbound = inbound(&workspace, &actor);
        let identity = ReceiverConversationIdentity::sms(workspace.id(), actor.user_id().clone());
        let accepted = db
            .accept_receiver_job(&inbound, &identity)
            .expect("accept receiver job");
        let scope = SessionScope::new(AgentKind::Codex, workspace.id(), actor.clone());
        let main = AgentSession::new("interactive-native-session").expect("main session");
        SessionStore::register(&db, &main, "interactive-shell", 41, &scope)
            .expect("register main session");
        let services = services(db);
        let claimed = services
            .claim_receiver_run("receiver-claim", 1_000, 1_500)
            .expect("claim receiver job")
            .expect("ready receiver job");
        if matches!(
            failure,
            ReceiverLaunchFailure::Allocation | ReceiverLaunchFailure::Spawn
        ) {
            assert!(
                services
                    .prepare_receiver_launch(accepted.job_id(), "receiver-claim", 1_010)
                    .expect("prepare receiver launch")
            );
        }
        let remote = ReceiverRemoteSession::new("interactive-shell");
        ReceiverSessionRegistration::register_fresh(&services, &remote, 42, &scope)
            .expect("register fresh remote placeholder")
            .commit();
        let shutdowns = Arc::new(Mutex::new(0));
        let mut controller = AgentController::new(
            Arc::clone(&workspace),
            actor,
            Box::new(LaunchFrontend),
            Box::new(ShutdownTransport(Arc::clone(&shutdowns))),
        );

        let outcome = rollback_receiver_launch(
            &services,
            &claimed,
            remote.instance(),
            &mut controller,
            failure,
            1_020,
            2_000,
        )
        .expect("roll back receiver launch");

        assert_eq!(outcome, ReceiverLaunchRetryOutcome::Scheduled);
        assert_eq!(*shutdowns.lock().expect("shutdown count"), 1);
        assert!(
            services
                .locked_session_for_instance(remote.instance(), &scope)
                .is_none(),
            "{failure:?} must release the remote session owner"
        );
        assert_eq!(
            services
                .locked_session_for_instance("interactive-shell", &scope)
                .as_deref(),
            Some("interactive-native-session")
        );
        let retry = services
            .claim_receiver_run("retry-owner", 2_000, 2_500)
            .expect("claim due retry")
            .expect("durable retry remains ready");
        assert_eq!(retry.job().state(), ReceiverJobState::Retrying);
        assert_eq!(retry.job().last_error(), Some(failure.as_str()));
    }
}
