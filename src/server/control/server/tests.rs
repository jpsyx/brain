use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::*;
use crate::server::lifecycle::{
    IngressId, LeaseAction, LeaseId, WorkspaceAvailability, WorkspaceLease,
};
use crate::server::workspace_route::{WorkspaceContextLoader, WorkspaceRouteError};
use crate::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, WorkspaceContext, WorkspaceId, WorkspaceRecord,
};

mod receiver_admission;

#[test]
fn refresh_enabled_reloads_persistent_intent_for_the_exact_live_workspace() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&registry_with_receiver(false))
        .expect("persist receiver intent");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(generation, store, fixture.path().to_path_buf());
    server
        .leases
        .register(lease(now + Duration::from_secs(30)), now)
        .expect("register enabled fixture lease");

    assert!(matches!(
        server.apply(
            ControlRequest::RefreshEnabled {
                generation,
                workspace_id: workspace_id(),
            },
            now,
        ),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    assert_eq!(
        server.leases.availability(ingress(), now),
        WorkspaceAvailability::Disabled
    );
}

#[test]
fn blocked_context_load_does_not_block_control_and_stale_ticket_cannot_route() {
    let now = Instant::now();
    let lease = lease(now + Duration::from_secs(30));
    let ingress = lease.ingress_id;
    let lease_id = lease.lease_id;
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(
        generation,
        RegistryStore::from_path(PathBuf::from("/unused/env.json")),
        PathBuf::from("/tmp"),
    );
    server
        .leases
        .apply(LeaseAction::Register { lease, now })
        .expect("register route fixture");
    let control = Arc::new(Mutex::new(server));
    let (load_started_tx, load_started_rx) = mpsc::sync_channel(0);
    let (release_load_tx, release_load_rx) = mpsc::sync_channel(0);
    let loader = BlockingLoader {
        load_started: load_started_tx,
        release_load: release_load_rx,
        context: workspace_context(),
    };

    let route_control = Arc::clone(&control);
    let route = std::thread::spawn(move || {
        crate::server::resolve_workspace_route_with_loader(&route_control, ingress, || now, &loader)
    });
    load_started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("route loader must start");

    let responsive_control = Arc::clone(&control);
    let (control_done_tx, control_done_rx) = mpsc::sync_channel(0);
    std::thread::spawn(move || {
        let mut server = responsive_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = server.apply(ControlRequest::Snapshot, now);
        let heartbeat = server.apply(
            ControlRequest::Heartbeat {
                generation,
                lease_id,
            },
            now,
        );
        let unregister = server.apply(
            ControlRequest::Unregister {
                generation,
                lease_id,
            },
            now,
        );
        drop(server);
        control_done_tx
            .send((snapshot, heartbeat, unregister))
            .expect("report control results");
    });
    let (snapshot, heartbeat, unregister) = control_done_rx
        .recv_timeout(Duration::from_millis(500))
        .expect("filesystem loading must not hold the control mutex");
    assert!(matches!(snapshot, ControlResponse::Snapshot(_)));
    assert!(matches!(
        heartbeat,
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    assert!(matches!(
        unregister,
        ControlResponse::Accepted { shutdown: true, .. }
    ));

    release_load_tx.send(()).expect("release route loader");
    let error = route
        .join()
        .expect("route thread")
        .expect_err("an unregistered ticket must not route");
    assert_eq!(error.status(), 503);
}

#[test]
fn disable_then_enable_cannot_revive_a_captured_route_ticket() {
    let now = Instant::now();
    let route_lease = lease(now + Duration::from_secs(30));
    let lease_id = route_lease.lease_id;
    let generation = ServerGeneration::new();
    let mut server = control_with_lease(generation, route_lease, now);
    let (ticket, _) = server
        .begin_workspace_route(ingress(), now)
        .expect("capture accepting route");

    server
        .leases
        .set_receiver_enabled(lease_id, false, now)
        .expect("disable receiver");
    server
        .leases
        .set_receiver_enabled(lease_id, true, now)
        .expect("re-enable receiver");

    let error = server
        .finish_workspace_route(&ticket, workspace_context(), now)
        .expect_err("revoked route incarnation must stay stale after re-enable");
    assert_eq!(error.status(), 503);
}

#[test]
fn same_id_reregistration_cannot_revive_a_captured_route_ticket() {
    let now = Instant::now();
    let route_lease = lease(now + Duration::from_secs(30));
    let lease_id = route_lease.lease_id;
    let generation = ServerGeneration::new();
    let mut server = control_with_lease(generation, route_lease.clone(), now);
    let (ticket, _) = server
        .begin_workspace_route(ingress(), now)
        .expect("capture accepting route");

    assert_eq!(
        server.leases.unregister(lease_id, now),
        crate::server::lifecycle::ServerDecision::ShutdownNow
    );
    server
        .leases
        .register(route_lease, now)
        .expect("re-register identical authority fields");

    let error = server
        .finish_workspace_route(&ticket, workspace_context(), now)
        .expect_err("new registration incarnation must reject the old ticket");
    assert_eq!(error.status(), 503);
}

#[test]
fn heartbeat_renewal_preserves_a_captured_route_ticket() {
    let now = Instant::now();
    let route_lease = lease(now + Duration::from_secs(30));
    let lease_id = route_lease.lease_id;
    let generation = ServerGeneration::new();
    let mut server = control_with_lease(generation, route_lease, now);
    let (ticket, _) = server
        .begin_workspace_route(ingress(), now)
        .expect("capture accepting route");

    server
        .leases
        .heartbeat(
            lease_id,
            now + Duration::from_secs(1),
            crate::server::lifecycle::LeaseTiming::new(
                Duration::from_secs(1),
                Duration::from_secs(30),
            ),
        )
        .expect("renew route lease");

    server
        .finish_workspace_route(&ticket, workspace_context(), now + Duration::from_secs(1))
        .expect("ordinary heartbeat must preserve route authority");
}

#[test]
fn disable_after_actor_resolution_rejects_before_socket_handoff() {
    let now = Instant::now();
    let route_lease = lease(now + Duration::from_secs(30));
    let lease_id = route_lease.lease_id;
    let generation = ServerGeneration::new();
    let mut server = control_with_lease(generation, route_lease, now);
    let (ticket, _) = server
        .begin_workspace_route(ingress(), now)
        .expect("capture accepting route");
    let route = server
        .finish_workspace_route(&ticket, workspace_context(), now)
        .expect("resolve initial route");
    let control = Arc::new(Mutex::new(server));
    let actor_resolved = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let forwards = Arc::new(AtomicUsize::new(0));
    let mut pipeline = AuthorityPipeline {
        route: Some(route),
        control: Arc::clone(&control),
        actor_resolved: Arc::clone(&actor_resolved),
        release: Arc::clone(&release),
        forwards: Arc::clone(&forwards),
        now,
    };
    let worker =
        std::thread::spawn(move || crate::server::receiver::execute_pipeline(&mut pipeline));

    actor_resolved.wait();
    control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .leases
        .set_receiver_enabled(lease_id, false, now)
        .expect("disable exact live lease");
    release.wait();

    worker
        .join()
        .expect("dispatch thread")
        .expect_err("revoked route must reject before socket handoff");
    assert_eq!(forwards.load(Ordering::Acquire), 0);
}

struct AuthorityPipeline {
    route: Option<crate::server::workspace_route::ResolvedWorkspaceRoute>,
    control: Arc<Mutex<ControlServer>>,
    actor_resolved: Arc<Barrier>,
    release: Arc<Barrier>,
    forwards: Arc<AtomicUsize>,
    now: Instant,
}

impl crate::server::receiver::DispatchPipeline for AuthorityPipeline {
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
        self.actor_resolved.wait();
        self.release.wait();
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
        self.control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revalidate_workspace_route(workspace, self.now)
            .map_err(Into::into)
    }

    fn forward(&mut self, _workspace: &Self::Workspace, _job: &()) -> anyhow::Result<()> {
        self.forwards.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

struct BlockingLoader {
    load_started: mpsc::SyncSender<()>,
    release_load: mpsc::Receiver<()>,
    context: WorkspaceContext,
}

impl WorkspaceContextLoader for BlockingLoader {
    fn load(&self, _lease: &WorkspaceLease) -> Result<WorkspaceContext, WorkspaceRouteError> {
        self.load_started.send(()).expect("signal route load");
        self.release_load
            .recv()
            .expect("wait for route load release");
        Ok(self.context.clone())
    }
}

fn lease(expires_at: Instant) -> WorkspaceLease {
    WorkspaceLease {
        lease_id: LeaseId::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").unwrap(),
        workspace_id: workspace_id(),
        canonical_name: WorkspaceName::parse("personal").unwrap(),
        ingress_id: IngressId::parse("a4f0ec11-d121-4f58-aa44-2448ba427b76").unwrap(),
        tui_pid: std::process::id(),
        job_socket: PathBuf::from("/tmp/jobs.sock"),
        receiver_enabled: true,
        expires_at,
    }
}

fn ingress() -> IngressId {
    IngressId::parse("a4f0ec11-d121-4f58-aa44-2448ba427b76").unwrap()
}

fn control_with_lease(
    generation: ServerGeneration,
    route_lease: WorkspaceLease,
    now: Instant,
) -> ControlServer {
    let mut server = ControlServer::new(
        generation,
        RegistryStore::from_path(PathBuf::from("/unused/env.json")),
        PathBuf::from("/tmp"),
    );
    server
        .leases
        .register(route_lease, now)
        .expect("register route fixture");
    server
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("2174fb9d-ae76-4bde-a526-38ac43ebdf8f").unwrap()
}

fn workspace_context() -> WorkspaceContext {
    WorkspaceContext::new(
        Path::new("/tmp"),
        workspace_id(),
        WorkspaceName::parse("personal").unwrap(),
        Path::new("/tmp/workspace"),
        "tester",
        Path::new("/tmp"),
    )
    .unwrap()
}

fn registry_with_receiver(receiver_enabled: bool) -> MachineRegistry {
    let name = WorkspaceName::parse("personal").unwrap();
    MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: name.clone(),
        workspaces: BTreeMap::from([(
            name,
            WorkspaceRecord {
                workspace_id: workspace_id(),
                root: PathBuf::from("/tmp/workspace"),
                aliases: BTreeSet::new(),
                local_user_id: "tester".to_owned(),
                receiver_enabled,
                env: serde_json::Map::new(),
            },
        )]),
    }
}
