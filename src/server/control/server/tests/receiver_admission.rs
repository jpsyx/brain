use std::time::{Duration, Instant};

use super::*;
use crate::server::lifecycle::ServerGeneration;
use crate::workspace::RegistryStore;

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
            live_leases: 1,
            receiver_enabled: Some(false),
        }
    );
}

#[test]
fn workspace_status_is_a_non_mutating_generation_snapshot() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(
        generation,
        RegistryStore::from_path(fixture.path().join("env.json")),
        fixture.path().to_path_buf(),
    );
    server
        .leases
        .register(lease(now + Duration::from_secs(1)), now)
        .expect("register expiring fixture lease");
    let before = format!("{:#?}", server.leases);

    let response = server.apply(
        ControlRequest::WorkspaceStatus {
            generation,
            workspace_id: workspace_id(),
        },
        now + Duration::from_secs(1),
    );

    assert_eq!(
        response,
        ControlResponse::WorkspaceStatus {
            generation,
            live_leases: 0,
            receiver_enabled: None,
        }
    );
    assert_eq!(format!("{:#?}", server.leases), before);
    assert_eq!(
        server.leases.expire(now + Duration::from_secs(1)),
        crate::server::lifecycle::ServerDecision::ShutdownNow
    );
}

#[test]
fn process_snapshot_does_not_reap_or_latch_an_expired_final_lease() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(
        generation,
        RegistryStore::from_path(fixture.path().join("env.json")),
        fixture.path().to_path_buf(),
    );
    server
        .leases
        .register(lease(now + Duration::from_secs(1)), now)
        .expect("register expiring fixture lease");
    let before = format!("{:#?}", server.leases);

    let response = server.apply(ControlRequest::Snapshot, now + Duration::from_secs(1));

    assert_eq!(
        response,
        ControlResponse::Snapshot(ServerSnapshot {
            generation,
            live_leases: 0,
        })
    );
    assert_eq!(format!("{:#?}", server.leases), before);
    assert_eq!(
        server.leases.expire(now + Duration::from_secs(1)),
        crate::server::lifecycle::ServerDecision::ShutdownNow
    );
}
