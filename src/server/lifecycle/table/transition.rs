//! Pure authority revision and registration identity helpers.

use std::collections::HashMap;

use crate::workspace::WorkspaceId;

use super::super::decision::{AuthorityChange, AuthorityRevision, authority_revision_after};
use super::super::{LeaseError, ServerDecision, WorkspaceLease};

pub(super) fn next_authority_revision(
    revisions: &HashMap<WorkspaceId, AuthorityRevision>,
    workspace_id: WorkspaceId,
) -> Result<AuthorityRevision, LeaseError> {
    authority_revision_after(
        revisions.get(&workspace_id).copied(),
        AuthorityChange::Revoked,
    )
    .ok_or(LeaseError::AuthorityRevisionOverflow)
}

pub(super) fn preserve_authority(
    revisions: &mut HashMap<WorkspaceId, AuthorityRevision>,
    workspace_id: WorkspaceId,
) {
    let revision = revisions.get(&workspace_id).copied();
    if let Some(preserved) = authority_revision_after(revision, AuthorityChange::Heartbeat) {
        revisions.insert(workspace_id, preserved);
    }
}

pub(super) fn same_registration(existing: &WorkspaceLease, replay: &WorkspaceLease) -> bool {
    existing.lease_id == replay.lease_id
        && existing.workspace_id == replay.workspace_id
        && existing.canonical_name == replay.canonical_name
        && existing.ingress_id == replay.ingress_id
        && existing.tui_pid == replay.tui_pid
}

pub(super) const fn shutdown_decision(removed_lease: bool, no_live_leases: bool) -> ServerDecision {
    if removed_lease && no_live_leases {
        ServerDecision::ShutdownNow
    } else {
        ServerDecision::KeepRunning
    }
}
