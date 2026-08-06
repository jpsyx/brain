use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::fixtures::{ingress, lease, registry_with_receiver, workspace_id};
use crate::server::control::{ControlRequest, ControlResponse, ControlServer};
use crate::server::lifecycle::{ServerGeneration, WorkspaceAvailability};
use crate::workspace::RegistryStore;

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
fn blocked_intent_io_never_holds_shared_control_state() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&registry_with_receiver(true))
        .expect("persist receiver intent");
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(generation, store, fixture.path().to_path_buf());
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = Arc::new(Mutex::new(release_rx));
    server.set_io_gate(Arc::new(move || {
        started_tx.send(()).expect("signal blocked intent load");
        release_rx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv_timeout(Duration::from_secs(1))
            .expect("release blocked intent load");
    }));
    let shared = Arc::new(Mutex::new(server));
    let worker_shared = Arc::clone(&shared);
    let worker = std::thread::spawn(move || {
        ControlServer::apply_shared_until(
            &worker_shared,
            ControlRequest::RefreshEnabled {
                generation,
                workspace_id: workspace_id(),
            },
            Instant::now(),
            Instant::now() + Duration::from_secs(2),
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("intent load entered");

    let snapshot_shared = Arc::clone(&shared);
    let (snapshot_tx, snapshot_rx) = mpsc::sync_channel(1);
    let snapshot_worker = std::thread::spawn(move || {
        snapshot_tx
            .send(ControlServer::apply_shared_until(
                &snapshot_shared,
                ControlRequest::Snapshot,
                Instant::now(),
                Instant::now() + Duration::from_millis(250),
            ))
            .expect("report snapshot");
    });
    let snapshot = snapshot_rx.recv_timeout(Duration::from_millis(500));
    release_tx.send(()).expect("release intent load");
    snapshot_worker.join().expect("snapshot worker");
    assert!(
        matches!(snapshot, Ok(ControlResponse::Snapshot(_))),
        "snapshot was blocked behind intent IO"
    );

    assert!(matches!(
        worker.join().expect("intent worker"),
        ControlResponse::Accepted { .. }
    ));
}

#[test]
fn deadline_crossing_during_intent_load_cannot_mutate_authority() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let store = RegistryStore::from_path(fixture.path().join("env.json"));
    store
        .replace(&registry_with_receiver(false))
        .expect("persist disabled receiver intent");
    let started = Instant::now();
    let deadline = started + Duration::from_secs(1);
    let generation = ServerGeneration::new();
    let mut server = ControlServer::new(generation, store, fixture.path().to_path_buf());
    server
        .leases
        .register(lease(started + Duration::from_secs(30)), started)
        .expect("register enabled lease");
    let shared = Arc::new(Mutex::new(server));
    let instants = Mutex::new(std::collections::VecDeque::from([started, deadline]));
    let clock = || {
        instants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or(deadline)
    };

    let response = ControlServer::apply_shared_until_with_clock(
        &shared,
        ControlRequest::RefreshEnabled {
            generation,
            workspace_id: workspace_id(),
        },
        started,
        deadline,
        &clock,
    );

    assert!(matches!(response, ControlResponse::Rejected { .. }));
    assert!(matches!(
        shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .leases
            .availability(ingress(), started),
        WorkspaceAvailability::Accepting(_)
    ));
}

#[test]
fn committed_admission_revocation_stops_at_deadline_without_later_mutation() {
    let fixture = tempfile::tempdir().expect("registry fixture");
    let started = Instant::now();
    let generation = ServerGeneration::new();
    let route_lease = lease(started + Duration::from_secs(30));
    let lease_id = route_lease.lease_id;
    let admission = Arc::new(crate::server::receiver::admission::ReceiverAdmission::new(
        route_lease.workspace_id,
        lease_id,
    ));
    admission.authorize().expect("authorize admission");
    admission.commit().expect("commit admission");
    let mut server = ControlServer::new(
        generation,
        RegistryStore::from_path(fixture.path().join("env.json")),
        fixture.path().to_path_buf(),
    );
    server
        .leases
        .register(route_lease, started)
        .expect("register lease");
    server.admissions.push(Arc::downgrade(&admission));
    let shared = Arc::new(Mutex::new(server));
    let worker_shared = Arc::clone(&shared);
    let (response_tx, response_rx) = mpsc::sync_channel(1);
    let deadline = Instant::now() + Duration::from_millis(20);
    let worker = std::thread::spawn(move || {
        response_tx
            .send(ControlServer::apply_shared_until(
                &worker_shared,
                ControlRequest::Unregister {
                    generation,
                    lease_id,
                },
                started,
                deadline,
            ))
            .expect("report bounded revocation");
    });

    let response = response_rx.recv_timeout(Duration::from_millis(250));
    admission.complete();
    worker.join().expect("revocation worker");
    let response = response.expect("revocation exceeded the absolute control deadline");

    assert!(matches!(response, ControlResponse::Rejected { .. }));
    assert!(matches!(
        shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .leases
            .availability(ingress(), started),
        WorkspaceAvailability::Accepting(_)
    ));
}
