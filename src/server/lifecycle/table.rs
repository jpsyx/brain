//! Pure lease registration, expiry, and ingress-routing decisions.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Instant;

use crate::workspace::WorkspaceId;

use super::{
    IngressId, LeaseId, LeaseTiming, ServerDecision, WorkspaceAvailability, WorkspaceLease,
};

/// A pure transition request for [`LeaseTable`].
#[derive(Debug, Clone)]
pub enum LeaseAction {
    /// Add one newly verified TUI lease.
    Register { lease: WorkspaceLease, now: Instant },
    /// Renew one existing lease using the supplied testable timing policy.
    Heartbeat {
        lease_id: LeaseId,
        now: Instant,
        timing: LeaseTiming,
    },
    /// Update receiver enablement for one existing live lease.
    SetReceiverEnabled {
        lease_id: LeaseId,
        receiver_enabled: bool,
        now: Instant,
    },
    /// Remove one orderly TUI lease.
    Unregister { lease_id: LeaseId, now: Instant },
    /// Reap expired leases, normally from the later watchdog.
    Expire { now: Instant },
}

/// State machine that keeps live TUI leases and remembered ingress identities.
#[derive(Debug, Default)]
pub struct LeaseTable {
    live: HashMap<WorkspaceId, WorkspaceLease>,
    known_ingresses: HashMap<IngressId, WorkspaceId>,
    known_workspace_ingresses: HashMap<WorkspaceId, IngressId>,
}

impl LeaseTable {
    /// Apply one lease transition.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseError`] for duplicate, stale, or overflowed transitions.
    pub fn apply(&mut self, action: LeaseAction) -> Result<ServerDecision, LeaseError> {
        match action {
            LeaseAction::Register { lease, now } => {
                self.register(lease, now)?;
                Ok(ServerDecision::KeepRunning)
            }
            LeaseAction::Heartbeat {
                lease_id,
                now,
                timing,
            } => {
                self.heartbeat(lease_id, now, timing)?;
                Ok(ServerDecision::KeepRunning)
            }
            LeaseAction::SetReceiverEnabled {
                lease_id,
                receiver_enabled,
                now,
            } => {
                self.set_receiver_enabled(lease_id, receiver_enabled, now)?;
                Ok(ServerDecision::KeepRunning)
            }
            LeaseAction::Unregister { lease_id, now } => Ok(self.unregister(lease_id, now)),
            LeaseAction::Expire { now } => Ok(self.expire(now)),
        }
    }

    /// Register one verified TUI lease.
    ///
    /// A stale previous lease for this same workspace is reaped before this
    /// check. A live duplicate lease or an ingress collision is rejected.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseError`] when the lease would violate workspace or ingress
    /// identity ownership.
    pub fn register(&mut self, lease: WorkspaceLease, now: Instant) -> Result<(), LeaseError> {
        self.prune_expired(now);

        if self.live.contains_key(&lease.workspace_id) {
            return Err(LeaseError::WorkspaceAlreadyLeased {
                workspace_id: lease.workspace_id,
            });
        }
        if self
            .live
            .values()
            .any(|existing| existing.ingress_id == lease.ingress_id)
        {
            return Err(LeaseError::IngressAlreadyLeased {
                ingress_id: lease.ingress_id,
            });
        }
        if self
            .known_ingresses
            .get(&lease.ingress_id)
            .is_some_and(|workspace_id| *workspace_id != lease.workspace_id)
        {
            return Err(LeaseError::IngressAlreadyKnown {
                ingress_id: lease.ingress_id,
            });
        }
        if self
            .known_workspace_ingresses
            .get(&lease.workspace_id)
            .is_some_and(|ingress_id| *ingress_id != lease.ingress_id)
        {
            return Err(LeaseError::WorkspaceIngressMismatch {
                workspace_id: lease.workspace_id,
            });
        }

        self.known_ingresses
            .insert(lease.ingress_id, lease.workspace_id);
        self.known_workspace_ingresses
            .insert(lease.workspace_id, lease.ingress_id);
        self.live.insert(lease.workspace_id, lease);
        Ok(())
    }

    /// Renew only the live lease identified by `lease_id`.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseError::LeaseNotLive`] when the lease was missing or had
    /// already expired, or [`LeaseError::ExpiryOverflow`] when a deadline
    /// cannot be represented.
    pub fn heartbeat(
        &mut self,
        lease_id: LeaseId,
        now: Instant,
        timing: LeaseTiming,
    ) -> Result<(), LeaseError> {
        self.prune_expired(now);
        let expires_at = now
            .checked_add(timing.ttl())
            .ok_or(LeaseError::ExpiryOverflow)?;
        let lease = self
            .live
            .values_mut()
            .find(|lease| lease.lease_id == lease_id)
            .ok_or(LeaseError::LeaseNotLive { lease_id })?;
        lease.expires_at = expires_at;
        Ok(())
    }

    /// Change receiver enablement for an existing live lease.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseError::LeaseNotLive`] when the lease was missing or
    /// expired.
    pub fn set_receiver_enabled(
        &mut self,
        lease_id: LeaseId,
        receiver_enabled: bool,
        now: Instant,
    ) -> Result<(), LeaseError> {
        self.prune_expired(now);
        let lease = self
            .live
            .values_mut()
            .find(|lease| lease.lease_id == lease_id)
            .ok_or(LeaseError::LeaseNotLive { lease_id })?;
        lease.receiver_enabled = receiver_enabled;
        Ok(())
    }

    /// Remove one orderly lease and decide whether the process must exit.
    #[must_use]
    pub fn unregister(&mut self, lease_id: LeaseId, now: Instant) -> ServerDecision {
        let expired = self.prune_expired(now);
        let removed = self
            .live
            .iter()
            .find_map(|(workspace_id, lease)| (lease.lease_id == lease_id).then_some(*workspace_id))
            .and_then(|workspace_id| self.live.remove(&workspace_id))
            .is_some();
        shutdown_decision(expired || removed, self.live.is_empty())
    }

    /// Reap expired leases and decide whether the final lease was lost.
    #[must_use]
    pub fn expire(&mut self, now: Instant) -> ServerDecision {
        shutdown_decision(self.prune_expired(now), self.live.is_empty())
    }

    /// List each live workspace in canonical-name order after reaping expiry.
    #[must_use]
    pub fn live_workspaces(&mut self, now: Instant) -> Vec<WorkspaceId> {
        self.prune_expired(now);
        let mut leases = self.live.values().collect::<Vec<_>>();
        leases.sort_unstable_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
        leases.into_iter().map(|lease| lease.workspace_id).collect()
    }

    /// Return routing availability without ever exposing an expired lease.
    #[must_use]
    pub fn availability(&mut self, ingress_id: IngressId, now: Instant) -> WorkspaceAvailability {
        self.prune_expired(now);
        match self
            .live
            .values()
            .find(|lease| lease.ingress_id == ingress_id)
        {
            Some(lease) if lease.receiver_enabled => {
                WorkspaceAvailability::Accepting(lease.clone())
            }
            Some(_) => WorkspaceAvailability::Disabled,
            None if self.known_ingresses.contains_key(&ingress_id) => {
                WorkspaceAvailability::NoLiveTui
            }
            None => WorkspaceAvailability::Unknown,
        }
    }

    fn prune_expired(&mut self, now: Instant) -> bool {
        let expired = self
            .live
            .iter()
            .filter_map(|(workspace_id, lease)| (lease.expires_at <= now).then_some(*workspace_id))
            .collect::<Vec<_>>();
        let removed_any = !expired.is_empty();
        for workspace_id in expired {
            self.live.remove(&workspace_id);
        }
        removed_any
    }
}

fn shutdown_decision(removed_lease: bool, no_live_leases: bool) -> ServerDecision {
    if removed_lease && no_live_leases {
        ServerDecision::ShutdownNow
    } else {
        ServerDecision::KeepRunning
    }
}

/// A rejected lease state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    /// A different live lease already owns the workspace.
    WorkspaceAlreadyLeased { workspace_id: WorkspaceId },
    /// A different live lease already owns the ingress.
    IngressAlreadyLeased { ingress_id: IngressId },
    /// A previously known ingress belongs to another workspace.
    IngressAlreadyKnown { ingress_id: IngressId },
    /// A workspace attempted to change its stable ingress identity.
    WorkspaceIngressMismatch { workspace_id: WorkspaceId },
    /// A heartbeat or update named no live lease.
    LeaseNotLive { lease_id: LeaseId },
    /// Renewing a deadline exceeded the monotonic clock range.
    ExpiryOverflow,
}

impl Display for LeaseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkspaceAlreadyLeased { workspace_id } => {
                write!(
                    formatter,
                    "workspace {workspace_id} already has a live lease"
                )
            }
            Self::IngressAlreadyLeased { ingress_id } => {
                write!(formatter, "ingress {ingress_id} already has a live lease")
            }
            Self::IngressAlreadyKnown { ingress_id } => {
                write!(
                    formatter,
                    "ingress {ingress_id} belongs to another workspace"
                )
            }
            Self::WorkspaceIngressMismatch { workspace_id } => {
                write!(
                    formatter,
                    "workspace {workspace_id} cannot change its ingress ID"
                )
            }
            Self::LeaseNotLive { lease_id } => write!(formatter, "lease {lease_id} is not live"),
            Self::ExpiryOverflow => {
                formatter.write_str("lease expiry exceeds the monotonic clock range")
            }
        }
    }
}

impl Error for LeaseError {}
