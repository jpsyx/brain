//! Immutable lease-table projections for lifecycle status commands.

use std::time::Instant;

use super::LeaseTable;
use crate::workspace::WorkspaceId;

/// Non-mutating projection of process and exact-workspace lease state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LeaseStatusView {
    pub(crate) live_leases: usize,
    pub(crate) receiver_enabled: Option<bool>,
}

impl LeaseTable {
    /// Observe unexpired leases without pruning or advancing lifecycle state.
    #[must_use]
    pub(crate) fn status_view(&self, workspace_id: WorkspaceId, now: Instant) -> LeaseStatusView {
        LeaseStatusView {
            live_leases: self.live_count_at(now),
            receiver_enabled: self
                .live
                .get(&workspace_id)
                .filter(|lease| lease.expires_at > now)
                .map(|lease| lease.receiver_enabled),
        }
    }

    /// Count unexpired leases without pruning or advancing lifecycle state.
    #[must_use]
    pub(crate) fn live_count_at(&self, now: Instant) -> usize {
        self.live
            .values()
            .filter(|lease| lease.expires_at > now)
            .count()
    }
}
