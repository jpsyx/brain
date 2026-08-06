use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::super::AuthorityRevision;
use super::super::{
    IngressId, LeaseAction, LeaseError, LeaseId, LeaseTable, LeaseTiming, ServerDecision,
    WorkspaceAvailability, WorkspaceLease,
};
use crate::workspace::{WorkspaceId, WorkspaceName};

const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
const FAMILY_INGRESS: &str = "57b162df-983a-45c3-ac7e-bad94eb27a99";

#[test]
fn late_heartbeat_cannot_consume_final_expiry_shutdown() {
    let now = Instant::now();
    let mut table = table_with_final_lease(now);

    assert!(matches!(
        table.apply(LeaseAction::Heartbeat {
            lease_id: lease_id(1),
            now: now + Duration::from_secs(5),
            timing: LeaseTiming::new(Duration::from_secs(1), Duration::from_secs(5)),
        }),
        Err(LeaseError::LeaseNotLive { .. })
    ));
    assert_eq!(
        table
            .apply(LeaseAction::Expire {
                now: now + Duration::from_secs(5),
            })
            .unwrap(),
        ServerDecision::ShutdownNow
    );
}

#[test]
fn late_receiver_update_cannot_consume_final_expiry_shutdown() {
    let now = Instant::now();
    let mut table = table_with_final_lease(now);

    assert!(matches!(
        table.apply(LeaseAction::SetReceiverEnabled {
            lease_id: lease_id(1),
            receiver_enabled: false,
            now: now + Duration::from_secs(5),
        }),
        Err(LeaseError::LeaseNotLive { .. })
    ));
    assert_eq!(
        table
            .apply(LeaseAction::Expire {
                now: now + Duration::from_secs(5),
            })
            .unwrap(),
        ServerDecision::ShutdownNow
    );
}

#[test]
fn rejected_registration_cannot_consume_final_expiry_shutdown() {
    let now = Instant::now();
    let mut table = table_with_final_lease(now);
    let replacement = lease(
        "personal",
        PERSONAL_ID,
        FAMILY_INGRESS,
        lease_id(2),
        now + Duration::from_secs(5),
        true,
    );

    assert!(matches!(
        table.apply(LeaseAction::Register {
            lease: replacement,
            now: now + Duration::from_secs(5),
        }),
        Err(LeaseError::IngressAlreadyLeased { .. })
    ));
    assert_eq!(
        table
            .apply(LeaseAction::Expire {
                now: now + Duration::from_secs(5),
            })
            .unwrap(),
        ServerDecision::ShutdownNow
    );
}

#[test]
fn receiver_enablement_overflow_leaves_the_entire_lease_table_unchanged() {
    let now = Instant::now();
    let mut table = table_with_final_lease(now);
    table
        .authority_revisions
        .insert(family_id(), AuthorityRevision::from_raw(u64::MAX));
    let accepting = table_snapshot(&table);

    assert!(matches!(
        table.set_receiver_enabled(lease_id(1), false, now),
        Err(LeaseError::AuthorityRevisionOverflow)
    ));
    assert_eq!(table_snapshot(&table), accepting);
    assert!(matches!(
        table.availability(ingress(FAMILY_INGRESS), now),
        WorkspaceAvailability::Accepting(_)
    ));

    table.live.get_mut(&family_id()).unwrap().receiver_enabled = false;
    let disabled = table_snapshot(&table);
    assert!(matches!(
        table.set_receiver_enabled(lease_id(1), true, now),
        Err(LeaseError::AuthorityRevisionOverflow)
    ));
    assert_eq!(table_snapshot(&table), disabled);
    assert_eq!(
        table.availability(ingress(FAMILY_INGRESS), now),
        WorkspaceAvailability::Disabled
    );
}

#[test]
fn receiver_changing_replay_overflow_cannot_extend_pre_revocation_authority() {
    let now = Instant::now();
    let mut table = table_with_final_lease(now);
    table
        .authority_revisions
        .insert(family_id(), AuthorityRevision::from_raw(u64::MAX));
    let mut replay = lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, false);
    replay.expires_at = now + Duration::from_secs(30);
    let before = table_snapshot(&table);

    assert!(matches!(
        table.register(replay, now),
        Err(LeaseError::AuthorityRevisionOverflow)
    ));
    assert_eq!(table_snapshot(&table), before);
    assert_eq!(
        table.availability(ingress(FAMILY_INGRESS), now + Duration::from_secs(6)),
        WorkspaceAvailability::NoLiveTui
    );
}

#[test]
fn status_view_filters_expiry_without_mutating_any_lease_table_state() {
    let now = Instant::now();
    let mut table = table_with_final_lease(now);
    let before = table_snapshot(&table);

    let status = table.status_view(family_id(), now + Duration::from_secs(5));

    assert_eq!(status.live_leases, 0);
    assert_eq!(status.receiver_enabled, None);
    assert_eq!(table_snapshot(&table), before);
    assert_eq!(
        table.expire(now + Duration::from_secs(5)),
        ServerDecision::ShutdownNow
    );
}

fn table_snapshot(table: &LeaseTable) -> String {
    format!("{table:#?}")
}

fn table_with_final_lease(now: Instant) -> LeaseTable {
    let mut table = LeaseTable::default();
    table
        .register(
            lease("family", FAMILY_ID, FAMILY_INGRESS, lease_id(1), now, true),
            now,
        )
        .unwrap();
    table
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
fn ingress(value: &str) -> IngressId {
    IngressId::parse(value).unwrap()
}
fn lease_id(last: u128) -> LeaseId {
    LeaseId::parse(&format!("00000000-0000-0000-0000-{last:012x}")).unwrap()
}
