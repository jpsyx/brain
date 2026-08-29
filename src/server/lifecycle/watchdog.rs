//! Clock-injected crashed-lease expiry for the shared process loop.

use std::time::{Duration, Instant};

use super::{LeaseAction, LeaseError, LeaseTable, ServerDecision};

/// Clock-driven lease expiry and initial-registration deadline.
pub(super) struct Watchdog {
    initial_registration_deadline: Instant,
}

impl Watchdog {
    /// Build a watchdog around an injected monotonic start time.
    pub(super) fn new(started_at: Instant, initial_registration_timeout: Duration) -> Self {
        Self {
            initial_registration_deadline: started_at + initial_registration_timeout,
        }
    }

    pub(super) fn background(started_at: Instant) -> Self {
        Self::new(started_at, Duration::from_secs(365 * 24 * 60 * 60 * 100))
    }

    /// Expire leases or stop an elected process whose caller never registers.
    pub(super) fn tick(
        &self,
        leases: &mut LeaseTable,
        now: Instant,
    ) -> Result<ServerDecision, LeaseError> {
        let decision = leases.apply(LeaseAction::Expire { now })?;
        if decision == ServerDecision::ShutdownNow
            || (leases.is_empty() && now >= self.initial_registration_deadline)
        {
            Ok(ServerDecision::ShutdownNow)
        } else {
            Ok(ServerDecision::KeepRunning)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::server::lifecycle::{IngressId, LeaseId, WorkspaceLease};
    use crate::workspace::{WorkspaceId, WorkspaceName};

    #[test]
    fn crashed_final_lease_runs_until_ttl_then_shuts_down_at_the_injected_clock() {
        let now = Instant::now();
        let watchdog = Watchdog::new(now, Duration::from_secs(30));
        let mut leases = LeaseTable::default();
        leases
            .register(
                WorkspaceLease {
                    lease_id: LeaseId::new(),
                    workspace_id: WorkspaceId::new(),
                    canonical_name: WorkspaceName::parse("personal").unwrap(),
                    ingress_id: IngressId::new(),
                    tui_pid: 42,
                    receiver_enabled: true,
                    expires_at: now + Duration::from_secs(5),
                },
                now,
            )
            .unwrap();

        assert_eq!(
            watchdog
                .tick(&mut leases, now + Duration::from_secs(4))
                .unwrap(),
            ServerDecision::KeepRunning
        );
        assert_eq!(
            watchdog
                .tick(&mut leases, now + Duration::from_secs(5))
                .unwrap(),
            ServerDecision::ShutdownNow
        );
        assert!(leases.is_empty());
    }

    #[test]
    fn injected_watchdog_clock_stops_a_process_that_never_registers() {
        let started_at = Instant::now();
        let watchdog = Watchdog::new(started_at, Duration::from_secs(5));
        let mut leases = LeaseTable::default();

        assert_eq!(
            watchdog.tick(&mut leases, started_at + Duration::from_secs(4)),
            Ok(ServerDecision::KeepRunning)
        );
        assert_eq!(
            watchdog.tick(&mut leases, started_at + Duration::from_secs(5)),
            Ok(ServerDecision::ShutdownNow)
        );
    }

    #[test]
    fn background_server_does_not_stop_before_a_tui_registers() {
        let started_at = Instant::now();
        let watchdog = Watchdog::background(started_at);
        let mut leases = LeaseTable::default();

        assert_eq!(
            watchdog.tick(&mut leases, started_at + Duration::from_secs(30)),
            Ok(ServerDecision::KeepRunning)
        );
    }
}
