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
    assert!(matches!(
        server.apply(ControlRequest::Register(fixture.registration()), now),
        ControlResponse::Rejected { .. }
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
