use super::support::{ControlFixture, generation, lease_id, stale_generation};
use brain::server::control::{
    ControlRequest, ControlResponse, ControlServer, HeartbeatDisposition, heartbeat_disposition,
};
use std::time::{Duration, Instant};

#[test]
fn heartbeat_recovery_is_required_for_missing_or_stale_generations() {
    assert_eq!(heartbeat_disposition(None), HeartbeatDisposition::Recover);
    assert_eq!(
        heartbeat_disposition(Some(&ControlResponse::StaleGeneration)),
        HeartbeatDisposition::Recover
    );
    assert_eq!(
        heartbeat_disposition(Some(&ControlResponse::Accepted {
            generation: generation(),
            shutdown: false,
        })),
        HeartbeatDisposition::Current
    );
}

#[test]
fn register_heartbeat_update_snapshot_and_unregister_are_generation_guarded() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(
        generation(),
        fixture.registry_store(),
        fixture.temporary.path().to_path_buf(),
    );
    let now = Instant::now();

    assert!(matches!(
        server.apply(ControlRequest::Register(fixture.registration()), now),
        ControlResponse::Accepted {
            generation: accepted_generation,
            shutdown: false,
        } if accepted_generation == generation()
    ));
    let lost_response = server.apply(ControlRequest::Register(fixture.registration()), now);
    assert!(matches!(
        lost_response,
        ControlResponse::Accepted {
            generation: accepted_generation,
            shutdown: false,
        } if accepted_generation == generation()
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::Heartbeat {
                generation: stale_generation(),
                lease_id: lease_id(),
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::StaleGeneration
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::Heartbeat {
                generation: generation(),
                lease_id: lease_id(),
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::UpdateEnabled {
                generation: generation(),
                lease_id: lease_id(),
                receiver_enabled: false,
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::Accepted {
            shutdown: false,
            ..
        }
    ));
    assert!(matches!(
        server.apply(ControlRequest::Snapshot, now + Duration::from_secs(1)),
        ControlResponse::Snapshot(snapshot)
            if snapshot.generation == generation() && snapshot.live_leases == 1
    ));
    assert!(matches!(
        server.apply(
            ControlRequest::Unregister {
                generation: generation(),
                lease_id: lease_id(),
            },
            now + Duration::from_secs(1),
        ),
        ControlResponse::Accepted { shutdown: true, .. }
    ));
}

#[test]
fn registration_retry_is_idempotent_only_after_the_same_response_is_lost() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(
        generation(),
        fixture.registry_store(),
        fixture.temporary.path().to_path_buf(),
    );
    let now = Instant::now();
    let registration = fixture.registration();

    let _lost_response = server.apply(ControlRequest::Register(registration.clone()), now);
    let retry = server.apply(
        ControlRequest::Register(registration.clone()),
        now + Duration::from_millis(1),
    );
    let mut competing_lease = registration.clone();
    competing_lease.lease_id =
        brain::server::lifecycle::LeaseId::parse("00000000-0000-0000-0000-000000000099")
            .expect("competing lease ID");
    let mut conflicting_identity = registration;
    conflicting_identity.tui_pid = conflicting_identity.tui_pid.saturating_add(1);

    assert!(matches!(
        retry,
        ControlResponse::Accepted {
            generation: accepted_generation,
            shutdown: false,
        } if accepted_generation == generation()
    ));
    assert!(matches!(
        server.apply(ControlRequest::Register(competing_lease), now),
        ControlResponse::Rejected { .. }
    ));
    assert!(matches!(
        server.apply(ControlRequest::Register(conflicting_identity), now),
        ControlResponse::Rejected { .. }
    ));
}

#[test]
fn live_workspace_ingress_lookup_is_generation_and_workspace_scoped() {
    let fixture = ControlFixture::new();
    let mut server = ControlServer::new(
        generation(),
        fixture.registry_store(),
        fixture.temporary.path().to_path_buf(),
    );
    let now = Instant::now();
    assert!(matches!(
        server.apply(ControlRequest::Register(fixture.registration()), now),
        ControlResponse::Accepted { .. }
    ));

    assert!(matches!(
        server.apply(
            ControlRequest::WorkspaceIngress {
                generation: stale_generation(),
                workspace_id: super::support::workspace_id(),
            },
            now,
        ),
        ControlResponse::StaleGeneration
    ));
    assert_eq!(
        server.apply(
            ControlRequest::WorkspaceIngress {
                generation: generation(),
                workspace_id: super::support::workspace_id(),
            },
            now,
        ),
        ControlResponse::WorkspaceIngress {
            generation: generation(),
            ingress_id: Some(fixture.ingress_id),
        }
    );
    assert_eq!(
        server.apply(
            ControlRequest::WorkspaceIngress {
                generation: generation(),
                workspace_id: brain::workspace::WorkspaceId::new(),
            },
            now,
        ),
        ControlResponse::WorkspaceIngress {
            generation: generation(),
            ingress_id: None,
        }
    );
}
