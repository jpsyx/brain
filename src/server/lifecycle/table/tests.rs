use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::super::{IngressId, LeaseId, LeaseTiming, WorkspaceAvailability, WorkspaceLease};
use super::{LeaseAction, LeaseError, LeaseTable, ServerDecision};
use crate::workspace::{WorkspaceId, WorkspaceName};

const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_INGRESS: &str = "57b162df-983a-45c3-ac7e-bad94eb27a99";
const PERSONAL_INGRESS: &str = "91a0cfc2-7427-49d5-a2f1-258f985cd7e5";

#[test]
fn apply_dispatches_every_lease_transition_and_final_shutdown() {
    let now = Instant::now();
    let timing = LeaseTiming::new(Duration::from_millis(5), Duration::from_secs(10));
    let mut table = LeaseTable::default();
    let family = lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true);
    let personal = lease(
        "personal",
        PERSONAL_ID,
        PERSONAL_INGRESS,
        lease_id(2),
        now,
        false,
    );

    assert_eq!(
        table
            .apply(LeaseAction::Register { lease: family, now })
            .unwrap(),
        ServerDecision::KeepRunning
    );
    assert_eq!(
        table
            .apply(LeaseAction::Register {
                lease: personal,
                now
            })
            .unwrap(),
        ServerDecision::KeepRunning
    );
    assert_eq!(table.live_workspaces(now), [family_id(), personal_id()]);
    assert_eq!(
        table
            .apply(LeaseAction::Heartbeat {
                lease_id: lease_id(1),
                now,
                timing
            })
            .unwrap(),
        ServerDecision::KeepRunning
    );
    assert_eq!(
        table
            .apply(LeaseAction::SetReceiverEnabled {
                lease_id: lease_id(2),
                receiver_enabled: true,
                now,
            })
            .unwrap(),
        ServerDecision::KeepRunning
    );
    assert_eq!(
        table
            .apply(LeaseAction::Unregister {
                lease_id: lease_id(1),
                now
            })
            .unwrap(),
        ServerDecision::KeepRunning
    );
    assert_eq!(
        table
            .apply(LeaseAction::Expire {
                now: now + Duration::from_secs(6)
            })
            .unwrap(),
        ServerDecision::ShutdownNow
    );
}

#[test]
fn rejects_duplicate_live_workspace_lease_and_ingress() {
    let now = Instant::now();
    let mut table = LeaseTable::default();
    table
        .register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
            now,
        )
        .unwrap();

    assert!(matches!(
        table.register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(2), now, true),
            now
        ),
        Err(LeaseError::WorkspaceAlreadyLeased { .. })
    ));
    assert!(matches!(
        table.register(
            lease(
                "personal",
                PERSONAL_ID,
                FAMILY_INGRESS,
                lease_id(3),
                now,
                true
            ),
            now
        ),
        Err(LeaseError::IngressAlreadyLeased { .. })
    ));
}

#[test]
fn rejects_a_duplicate_live_lease_id() {
    let now = Instant::now();
    let mut table = LeaseTable::default();
    table
        .register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
            now,
        )
        .unwrap();

    assert!(matches!(
        table.register(
            lease(
                "personal",
                PERSONAL_ID,
                PERSONAL_INGRESS,
                lease_id(1),
                now,
                true
            ),
            now
        ),
        Err(LeaseError::LeaseAlreadyLeased { .. })
    ));
}

#[test]
fn rejects_an_incoming_lease_that_is_already_expired_at_the_injected_clock() {
    let now = Instant::now();
    let mut table = LeaseTable::default();
    let mut expired = lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true);
    expired.expires_at = now;

    assert!(matches!(
        table.register(expired, now),
        Err(LeaseError::LeaseExpired { .. })
    ));
    assert_eq!(
        table.availability(ingress(FAMILY_INGRESS), now),
        WorkspaceAvailability::Unknown
    );
}

#[test]
fn heartbeat_renews_only_its_matching_live_lease_and_expiry_never_routes_stale_data() {
    let now = Instant::now();
    let timing = LeaseTiming::new(Duration::from_millis(5), Duration::from_secs(10));
    let mut table = LeaseTable::default();
    table
        .register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
            now,
        )
        .unwrap();
    table
        .register(
            lease(
                "personal",
                PERSONAL_ID,
                PERSONAL_INGRESS,
                lease_id(2),
                now,
                true,
            ),
            now,
        )
        .unwrap();

    table
        .heartbeat(lease_id(1), now + Duration::from_secs(1), timing)
        .unwrap();

    assert!(matches!(
        table.availability(ingress(FAMILY_INGRESS), now + Duration::from_secs(10)),
        WorkspaceAvailability::Accepting(_)
    ));
    assert_eq!(
        table.availability(ingress(PERSONAL_INGRESS), now + Duration::from_secs(10)),
        WorkspaceAvailability::NoLiveTui
    );
    assert_eq!(
        table.expire(now + Duration::from_secs(12)),
        ServerDecision::ShutdownNow
    );
}

#[test]
fn stale_same_workspace_lease_is_replaced_without_disturbing_another_live_workspace() {
    let now = Instant::now();
    let mut table = LeaseTable::default();
    table
        .register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
            now,
        )
        .unwrap();
    table
        .register(
            lease(
                "personal",
                PERSONAL_ID,
                PERSONAL_INGRESS,
                lease_id(3),
                now + Duration::from_secs(7),
                true,
            ),
            now,
        )
        .unwrap();

    table
        .register(
            lease(
                "family",
                FAMILY_ID,
                FAMILY_INGRESS,
                lease_id(2),
                now + Duration::from_secs(10),
                true,
            ),
            now + Duration::from_secs(10),
        )
        .unwrap();

    assert!(
        matches!(table.availability(ingress(FAMILY_INGRESS), now + Duration::from_secs(10)), WorkspaceAvailability::Accepting(WorkspaceLease { lease_id: found, .. }) if found == lease_id(2))
    );
    assert!(
        matches!(table.availability(ingress(PERSONAL_INGRESS), now + Duration::from_secs(10)), WorkspaceAvailability::Accepting(WorkspaceLease { lease_id: found, .. }) if found == lease_id(3))
    );
}

#[test]
fn disabled_no_live_tui_and_unknown_ingress_remain_distinct() {
    let now = Instant::now();
    let mut table = LeaseTable::default();
    table
        .register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, false),
            now,
        )
        .unwrap();

    assert_eq!(
        table.availability(ingress(FAMILY_INGRESS), now),
        WorkspaceAvailability::Disabled
    );
    assert_eq!(
        table.unregister(lease_id(1), now),
        ServerDecision::ShutdownNow
    );
    assert_eq!(
        table.availability(ingress(FAMILY_INGRESS), now),
        WorkspaceAvailability::NoLiveTui
    );
    assert_eq!(
        table.availability(ingress(PERSONAL_INGRESS), now),
        WorkspaceAvailability::Unknown
    );
}

fn lease(
    name: &str,
    workspace: &str,
    ingress_id: &str,
    id: LeaseId,
    now: Instant,
    enabled: bool,
) -> WorkspaceLease {
    WorkspaceLease {
        lease_id: id,
        workspace_id: WorkspaceId::parse(workspace).unwrap(),
        canonical_name: WorkspaceName::parse(name).unwrap(),
        ingress_id: ingress(ingress_id),
        tui_pid: 42,
        job_socket: PathBuf::from("/tmp/brain-job.sock"),
        receiver_enabled: enabled,
        expires_at: now + Duration::from_secs(5),
    }
}

fn family_id() -> WorkspaceId {
    WorkspaceId::parse(FAMILY_ID).unwrap()
}
fn personal_id() -> WorkspaceId {
    WorkspaceId::parse(PERSONAL_ID).unwrap()
}
fn ingress(value: &str) -> IngressId {
    IngressId::parse(value).unwrap()
}
fn lease_id(last: u128) -> LeaseId {
    LeaseId::parse(&format!("00000000-0000-0000-0000-{last:012x}")).unwrap()
}
