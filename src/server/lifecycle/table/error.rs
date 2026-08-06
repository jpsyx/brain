//! Rejected lease-table transitions.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::server::lifecycle::{IngressId, LeaseId};
use crate::workspace::WorkspaceId;

/// A rejected lease state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    /// A different live workspace already owns the lease ID.
    LeaseAlreadyLeased { lease_id: LeaseId },
    /// A different live lease already owns the workspace.
    WorkspaceAlreadyLeased { workspace_id: WorkspaceId },
    /// A different live lease already owns the ingress.
    IngressAlreadyLeased { ingress_id: IngressId },
    /// A previously known ingress belongs to another workspace.
    IngressAlreadyKnown { ingress_id: IngressId },
    /// A workspace attempted to change its stable ingress identity.
    WorkspaceIngressMismatch { workspace_id: WorkspaceId },
    /// Registration supplied a lease whose deadline is already elapsed.
    LeaseExpired { lease_id: LeaseId },
    /// A heartbeat or update named no live lease.
    LeaseNotLive { lease_id: LeaseId },
    /// Renewing a deadline exceeded the monotonic clock range.
    ExpiryOverflow,
    /// A workspace authority incarnation counter exhausted its range.
    AuthorityRevisionOverflow,
}

impl Display for LeaseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LeaseAlreadyLeased { lease_id } => {
                write!(
                    formatter,
                    "lease {lease_id} already belongs to a live workspace"
                )
            }
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
            Self::LeaseExpired { lease_id } => {
                write!(formatter, "lease {lease_id} is already expired")
            }
            Self::LeaseNotLive { lease_id } => write!(formatter, "lease {lease_id} is not live"),
            Self::ExpiryOverflow => {
                formatter.write_str("lease expiry exceeds the monotonic clock range")
            }
            Self::AuthorityRevisionOverflow => {
                formatter.write_str("workspace authority revision exceeds its range")
            }
        }
    }
}

impl Error for LeaseError {}
