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
#[path = "table/mutation.rs"]
mod mutation;
#[path = "table/status.rs"]
mod status;
pub(crate) use status::LeaseStatusView;
#[path = "table/transition.rs"]
mod transition;
use transition::shutdown_decision;

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
    inherited_capabilities: HashMap<WorkspaceId, LeaseId>,
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

    /// A local capability one live lease inherited from the browser-only
    /// background lease it superseded.
    ///
    /// `brain habits` hands the browser a page whose URL carries the lease that
    /// was live when it rendered. A TUI starting afterwards replaces that
    /// browser-only lease with its own, and the already-open page cannot know
    /// it. So the superseding lease keeps honoring exactly the one capability
    /// it took over, for as long as it is itself live; losing the workspace's
    /// live lease retires it with everything else.
    #[must_use]
    pub(crate) fn honors_local_capability(
        &self,
        workspace_id: WorkspaceId,
        capability: LeaseId,
    ) -> bool {
        self.inherited_capabilities
            .get(&workspace_id)
            .is_some_and(|inherited| *inherited == capability)
    }

    /// Remove one orderly lease and decide whether the process must exit.
    #[must_use]
    pub fn unregister(&mut self, lease_id: LeaseId, _now: Instant) -> ServerDecision {
        let removed = self
            .live
            .iter()
            .find_map(|(workspace_id, lease)| (lease.lease_id == lease_id).then_some(*workspace_id))
            .and_then(|workspace_id| {
                self.inherited_capabilities.remove(&workspace_id);
                self.live.remove(&workspace_id)
            })
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

    /// Return the exact live lease for a local capability route, regardless of
    /// the workspace's inbound receiver setting.
    #[must_use]
    pub(crate) fn live_local_lease(
        &self,
        ingress_id: IngressId,
        now: Instant,
    ) -> Option<WorkspaceLease> {
        self.live
            .values()
            .find(|lease| lease.ingress_id == ingress_id && lease.expires_at > now)
            .cloned()
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

    /// The ingress this process remembers for one workspace, live or expired.
    ///
    /// Provider routes select a workspace by the address a message arrived at,
    /// then resolve it exactly as an ingress-carrying URL used to: a remembered
    /// ingress is what separates "asleep" from "no such route".
    #[must_use]
    pub(crate) fn known_workspace_ingress(&self, workspace_id: WorkspaceId) -> Option<IngressId> {
        self.known_workspace_ingresses.get(&workspace_id).copied()
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
            self.inherited_capabilities.remove(&workspace_id);
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
