use std::time::Instant;

use crate::workspace::WorkspaceId;

use super::{LeaseError, LeaseTable};
use crate::server::lifecycle::table::transition::{
    next_authority_revision, preserve_authority, same_registration,
};
use crate::server::lifecycle::{LeaseId, LeaseTiming, WorkspaceLease};

impl LeaseTable {
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
        let mut superseded = None;
        if let Some(existing) = self.live.get_mut(&lease.workspace_id) {
            if existing.tui_pid == 0 && lease.tui_pid != 0 {
                superseded = self
                    .live
                    .remove(&lease.workspace_id)
                    .map(|background| background.lease_id);
            } else {
                if same_registration(existing, &lease) {
                    let next_revision = (existing.receiver_enabled != lease.receiver_enabled)
                        .then(|| {
                            next_authority_revision(&self.authority_revisions, lease.workspace_id)
                        })
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
        if let Some(superseded) = superseded {
            self.inherited_capabilities
                .insert(lease.workspace_id, superseded);
        }
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
}
