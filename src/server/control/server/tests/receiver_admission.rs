use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;

use super::*;
use crate::server::lifecycle::ServerGeneration;
use crate::workspace::{RegistryStore, WorkspaceName};

#[test]
fn workspace_status_reports_a_live_but_disabled_exact_lease() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&registry_with_receiver(true))
        .expect("persist enabled receiver intent");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(generation, store, fixture.path().to_path_buf());
    let mut disabled_lease = lease(now + Duration::from_secs(30));
    disabled_lease.receiver_enabled = false;
    server
        .leases
        .register(disabled_lease, now)
        .expect("register disabled fixture lease");

    assert_eq!(
        server.apply(
            ControlRequest::WorkspaceStatus {
                generation,
                workspace_id: workspace_id(),
            },
            now,
        ),
        ControlResponse::WorkspaceStatus {
            generation,
            receiver_enabled: Some(false),
        }
    );
}

#[test]
fn persisted_disable_after_context_load_rejects_before_socket_handoff() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&registry_with_receiver(true))
        .expect("persist enabled receiver intent");
    let socket_path = fixture.path().join("jobs.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("job listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let now = Instant::now();
    let mut route_lease = lease(now + Duration::from_secs(30));
    route_lease.job_socket.clone_from(&socket_path);
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(generation, store.clone(), fixture.path().to_path_buf());
    server
        .leases
        .register(route_lease, now)
        .expect("register route fixture");
    let (ticket, _) = server
        .begin_workspace_route(ingress(), now)
        .expect("capture accepting route");
    let route = server
        .finish_workspace_route(&ticket, workspace_context(), now)
        .expect("resolve initial route");
    let control = Arc::new(Mutex::new(server));
    let provider_finished = Arc::new(Barrier::new(2));
    let release_handoff = Arc::new(Barrier::new(2));
    let mut pipeline = PersistedIntentPipeline {
        route: Some(route),
        control: Arc::clone(&control),
        provider_finished: Arc::clone(&provider_finished),
        release_handoff: Arc::clone(&release_handoff),
        now,
    };
    let worker =
        std::thread::spawn(move || crate::server::receiver::execute_pipeline(&mut pipeline));

    provider_finished.wait();
    store
        .transition_receiver(
            &WorkspaceName::parse("personal").unwrap(),
            workspace_id(),
            crate::workspace::ReceiverAction::Stop,
        )
        .expect("persist disable without live refresh");
    release_handoff.wait();

    worker
        .join()
        .expect("dispatch thread")
        .expect_err("persisted disable must reject before socket handoff");
    assert!(matches!(
        listener
            .accept()
            .expect_err("disabled route must not connect")
            .kind(),
        std::io::ErrorKind::WouldBlock
    ));
}

struct PersistedIntentPipeline {
    route: Option<crate::server::workspace_route::ResolvedWorkspaceRoute>,
    control: Arc<Mutex<ControlServer>>,
    provider_finished: Arc<Barrier>,
    release_handoff: Arc<Barrier>,
    now: Instant,
}

impl crate::server::receiver::DispatchPipeline for PersistedIntentPipeline {
    type Workspace = crate::server::workspace_route::ResolvedWorkspaceRoute;
    type ProviderConfig = ();
    type Authenticated = ();
    type Actor = ();
    type Job = ();

    fn resolve_workspace(&mut self) -> anyhow::Result<Self::Workspace> {
        self.route.take().context("route was already consumed")
    }

    fn load_provider_config(&mut self, _workspace: &Self::Workspace) -> anyhow::Result<()> {
        Ok(())
    }

    fn verify_signature(&mut self, _config: &()) -> anyhow::Result<()> {
        Ok(())
    }

    fn resolve_actor(
        &mut self,
        _workspace: &Self::Workspace,
        _authenticated: &(),
    ) -> anyhow::Result<()> {
        self.provider_finished.wait();
        self.release_handoff.wait();
        Ok(())
    }

    fn build_job(
        &mut self,
        _workspace: &Self::Workspace,
        _actor: &(),
        _authenticated: &(),
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn revalidate_authority(
        &mut self,
        workspace: &Self::Workspace,
        _job: &(),
    ) -> anyhow::Result<()> {
        workspace.revalidate_receiver_intent()?;
        self.control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revalidate_workspace_route(workspace, self.now)
            .map_err(Into::into)
    }

    fn forward(&mut self, workspace: &Self::Workspace, _job: &()) -> anyhow::Result<()> {
        std::os::unix::net::UnixStream::connect(&workspace.lease().job_socket)?;
        Ok(())
    }
}
