//! Clock-injected crashed-lease expiry for the shared process loop.

use std::time::Instant;

use super::{LeaseAction, LeaseError, LeaseTable, ServerDecision};

/// Expire leases at one injected monotonic instant.
pub(super) fn tick(leases: &mut LeaseTable, now: Instant) -> Result<ServerDecision, LeaseError> {
    leases.apply(LeaseAction::Expire { now })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::server::lifecycle::{IngressId, LeaseId, WorkspaceLease};
    use crate::workspace::{WorkspaceId, WorkspaceName};

    #[test]
    fn injected_watchdog_clock_requests_shutdown_after_final_expiry() {
        let now = Instant::now();
        let mut leases = LeaseTable::default();
        leases
            .register(
                WorkspaceLease {
                    lease_id: LeaseId::new(),
                    workspace_id: WorkspaceId::new(),
                    canonical_name: WorkspaceName::parse("personal").unwrap(),
                    ingress_id: IngressId::new(),
                    tui_pid: 42,
                    job_socket: PathBuf::from("/tmp/job.sock"),
                    receiver_enabled: true,
                    expires_at: now + Duration::from_secs(5),
                },
                now,
            )
            .unwrap();

        assert_eq!(
            tick(&mut leases, now + Duration::from_secs(5)).unwrap(),
            ServerDecision::ShutdownNow
        );
    }
}
