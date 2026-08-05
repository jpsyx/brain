use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::*;
use crate::server::lifecycle::{IngressId, LeaseAction, LeaseId, WorkspaceLease};
use crate::server::workspace_route::{WorkspaceContextLoader, WorkspaceRouteError};
use crate::workspace::{WorkspaceContext, WorkspaceId};

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
