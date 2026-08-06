//! Pure lease registration, expiry, and ingress-routing decisions.

use std::collections::HashMap;
use std::time::Instant;

use crate::workspace::WorkspaceId;

use super::decision::AuthorityRevision;
use super::{
    IngressId, LeaseId, LeaseTiming, ServerDecision, WorkspaceAvailability, WorkspaceLease,
};

#[path = "table/error.rs"]
mod error;
pub use error::LeaseError;
#[path = "table/status.rs"]
mod status;
pub(crate) use status::LeaseStatusView;
#[path = "table/transition.rs"]
mod transition;
use transition::{
    next_authority_revision, preserve_authority, same_registration, shutdown_decision,
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
    authority_revisions: HashMap<WorkspaceId, AuthorityRevision>,
    shutdown_pending: bool,
}

impl LeaseTable {
    /// Whether no live TUI lease remains.
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

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
    /// A duplicate lease or ingress collision is rejected. Expiry removal is
    /// owned exclusively by the control-server watchdog transition.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseError`] when the lease would violate workspace or ingress
    /// identity ownership or its authority revision cannot advance.
    pub fn register(&mut self, lease: WorkspaceLease, now: Instant) -> Result<(), LeaseError> {
        if lease.expires_at <= now {
            return Err(LeaseError::LeaseExpired {
                lease_id: lease.lease_id,
            });
        }
        if let Some(existing) = self.live.get_mut(&lease.workspace_id) {
            if same_registration(existing, &lease) {
                let next_revision = (existing.receiver_enabled != lease.receiver_enabled)
                    .then(|| next_authority_revision(&self.authority_revisions, lease.workspace_id))
                    .transpose()?;
                existing.expires_at = lease.expires_at;
                existing.receiver_enabled = lease.receiver_enabled;
                if let Some(next_revision) = next_revision {
                    self.authority_revisions
                        .insert(lease.workspace_id, next_revision);
                }
                self.shutdown_pending = false;
                return Ok(());
            }
            return Err(LeaseError::WorkspaceAlreadyLeased {
                workspace_id: lease.workspace_id,
            });
        }
        if self
            .live
            .values()
            .any(|existing| existing.lease_id == lease.lease_id)
        {
            return Err(LeaseError::LeaseAlreadyLeased {
                lease_id: lease.lease_id,
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

        let next_revision = next_authority_revision(&self.authority_revisions, lease.workspace_id)?;
        self.known_ingresses
            .insert(lease.ingress_id, lease.workspace_id);
        self.known_workspace_ingresses
            .insert(lease.workspace_id, lease.ingress_id);
        self.authority_revisions
            .insert(lease.workspace_id, next_revision);
        self.live.insert(lease.workspace_id, lease);
        self.shutdown_pending = false;
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
        let expires_at = now
            .checked_add(timing.ttl())
            .ok_or(LeaseError::ExpiryOverflow)?;
        let lease = self
            .live
            .values_mut()
            .find(|lease| lease.lease_id == lease_id && lease.expires_at > now)
            .ok_or(LeaseError::LeaseNotLive { lease_id })?;
        let workspace_id = lease.workspace_id;
        lease.expires_at = expires_at;
        preserve_authority(&mut self.authority_revisions, workspace_id);
        Ok(())
    }

    /// Change receiver enablement for an existing live lease.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseError::LeaseNotLive`] when the lease was missing or
    /// expired, or [`LeaseError::AuthorityRevisionOverflow`] when the authority
    /// revision cannot advance. An overflow leaves the lease unchanged.
    pub fn set_receiver_enabled(
        &mut self,
        lease_id: LeaseId,
        receiver_enabled: bool,
        now: Instant,
    ) -> Result<(), LeaseError> {
        let lease = self
            .live
            .values_mut()
            .find(|lease| lease.lease_id == lease_id && lease.expires_at > now)
            .ok_or(LeaseError::LeaseNotLive { lease_id })?;
        if lease.receiver_enabled == receiver_enabled {
            return Ok(());
        }
        let workspace_id = lease.workspace_id;
        let next_revision = next_authority_revision(&self.authority_revisions, workspace_id)?;
        lease.receiver_enabled = receiver_enabled;
        self.authority_revisions.insert(workspace_id, next_revision);
        Ok(())
    }

    /// Refresh receiver intent for an exact workspace when its lease is live.
    ///
    /// A missing or expired lease is a successful no-op because persistent
    /// intent remains authoritative for the next registration.
    pub fn refresh_workspace_receiver_enabled(
        &mut self,
        workspace_id: WorkspaceId,
        receiver_enabled: bool,
        now: Instant,
    ) -> Result<(), LeaseError> {
        let Some(lease) = self.live.get_mut(&workspace_id) else {
            return Ok(());
        };
        if lease.expires_at <= now {
            return Ok(());
        }
        if lease.receiver_enabled == receiver_enabled {
            return Ok(());
        }
        let next_revision = next_authority_revision(&self.authority_revisions, workspace_id)?;
        lease.receiver_enabled = receiver_enabled;
        self.authority_revisions.insert(workspace_id, next_revision);
        Ok(())
    }

    /// Remove one orderly lease and decide whether the process must exit.
    #[must_use]
    pub fn unregister(&mut self, lease_id: LeaseId, _now: Instant) -> ServerDecision {
        let removed = self
            .live
            .iter()
            .find_map(|(workspace_id, lease)| (lease.lease_id == lease_id).then_some(*workspace_id))
            .and_then(|workspace_id| self.live.remove(&workspace_id))
            .is_some();
        shutdown_decision(self.shutdown_pending || removed, self.live.is_empty())
    }

    /// Reap expired leases and decide whether the final lease was lost.
    #[must_use]
    pub fn expire(&mut self, now: Instant) -> ServerDecision {
        let expired = self.prune_expired(now);
        shutdown_decision(self.shutdown_pending || expired, self.live.is_empty())
    }

    /// List each unexpired workspace in canonical-name order without mutation.
    #[must_use]
    pub fn live_workspaces(&self, now: Instant) -> Vec<WorkspaceId> {
        let mut leases = self
            .live
            .values()
            .filter(|lease| lease.expires_at > now)
            .collect::<Vec<_>>();
        leases.sort_unstable_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
        leases.into_iter().map(|lease| lease.workspace_id).collect()
    }

    /// Return only the accepted ingress of an exact live workspace lease.
    #[must_use]
    pub fn live_ingress(&self, workspace_id: WorkspaceId, now: Instant) -> Option<IngressId> {
        self.live
            .get(&workspace_id)
            .filter(|lease| lease.expires_at > now)
            .map(|lease| lease.ingress_id)
    }

    #[must_use]
    pub fn live_local_route(
        &self,
        workspace_id: WorkspaceId,
        now: Instant,
    ) -> Option<(IngressId, LeaseId)> {
        self.live
            .get(&workspace_id)
            .filter(|lease| lease.expires_at > now)
            .map(|lease| (lease.ingress_id, lease.lease_id))
    }

    /// Return the receiver snapshot of an exact unexpired workspace lease.
    #[must_use]
    pub fn live_receiver_enabled(&self, workspace_id: WorkspaceId, now: Instant) -> Option<bool> {
        self.live
            .get(&workspace_id)
            .filter(|lease| lease.expires_at > now)
            .map(|lease| lease.receiver_enabled)
    }

    /// Return routing availability without ever exposing an expired lease.
    #[must_use]
    pub fn availability(&self, ingress_id: IngressId, now: Instant) -> WorkspaceAvailability {
        match self
            .live
            .values()
            .find(|lease| lease.ingress_id == ingress_id && lease.expires_at > now)
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

    pub(crate) fn known_workspace(&self, ingress_id: IngressId) -> Option<WorkspaceId> {
        self.known_ingresses.get(&ingress_id).copied()
    }

    /// Exact authority incarnation for one current workspace lease.
    #[must_use]
    pub(crate) fn authority_revision(
        &self,
        workspace_id: WorkspaceId,
    ) -> Option<AuthorityRevision> {
        self.authority_revisions.get(&workspace_id).copied()
    }

    pub(crate) fn expired_lease_ids(&self, now: Instant) -> Vec<LeaseId> {
        self.live
            .values()
            .filter_map(|lease| (lease.expires_at <= now).then_some(lease.lease_id))
            .collect()
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
        if removed_any && self.live.is_empty() {
            self.shutdown_pending = true;
        }
        removed_any
    }
}

#[cfg(test)]
#[path = "table/tests.rs"]
mod tests;
