//! Which lease capabilities a local `/local/<lease>/...` route accepts.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::WorkspaceRouteAuthority;
use crate::server::lifecycle::{IngressId, LeaseId, LeaseTable, ServerGeneration, WorkspaceLease};
use crate::workspace::{WorkspaceId, WorkspaceName};

const FAMILY_ID: &str = "e806258e-491a-436d-9db4-a5ca9903e0d4";
const FAMILY_INGRESS: &str = "57b162df-983a-45c3-ac7e-bad94eb27a99";

/// `brain habits` opened a page under its browser-only lease; a TUI then
/// started and took the workspace over. The already-open page keeps posting
/// the capability it was rendered with, so it must keep working.
#[test]
fn a_page_opened_under_the_background_lease_survives_a_tui_takeover() {
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut table = LeaseTable::default();
    let mut background = lease(lease_id(1), now);
    background.tui_pid = 0;
    background.job_socket = PathBuf::new();
    table.register(background, now).unwrap();
    table.register(lease(lease_id(2), now), now).unwrap();

    let ticket =
        WorkspaceRouteAuthority::begin_local(&table, generation, ingress(), lease_id(1), now)
            .expect("the superseded browser capability still routes");

    assert_eq!(ticket.lease().lease_id, lease_id(2));
    WorkspaceRouteAuthority::finish_local(&table, generation, &ticket, now).unwrap();
}

#[test]
fn a_capability_that_never_owned_the_ingress_is_not_found() {
    let now = Instant::now();
    let generation = ServerGeneration::new();
    let mut table = LeaseTable::default();
    table.register(lease(lease_id(2), now), now).unwrap();

    let error =
        WorkspaceRouteAuthority::begin_local(&table, generation, ingress(), lease_id(9), now)
            .expect_err("a stranger's capability must not route");

    assert_eq!(error.status(), 404);
}

fn lease(id: LeaseId, now: Instant) -> WorkspaceLease {
    WorkspaceLease {
        lease_id: id,
        workspace_id: WorkspaceId::parse(FAMILY_ID).unwrap(),
        canonical_name: WorkspaceName::parse("family").unwrap(),
        ingress_id: ingress(),
        tui_pid: 42,
        job_socket: PathBuf::from("/tmp/brain-job.sock"),
        receiver_enabled: true,
        expires_at: now + Duration::from_secs(5),
    }
}

fn ingress() -> IngressId {
    IngressId::parse(FAMILY_INGRESS).unwrap()
}

fn lease_id(last: u128) -> LeaseId {
    LeaseId::parse(&format!("00000000-0000-0000-0000-{last:012x}")).unwrap()
}
