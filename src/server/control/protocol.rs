//! Wire types exchanged between interactive brain clients and the shared server.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::server::lifecycle::{IngressId, LeaseId, ServerGeneration};
use crate::workspace::WorkspaceId;

/// The validated identity and delivery endpoint of one live TUI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRegistration {
    /// Shared-server generation this registration targets.
    pub generation: ServerGeneration,
    /// Fresh identity for this TUI lifetime.
    pub lease_id: LeaseId,
    /// Stable workspace identity from the selected registry entry.
    pub workspace_id: WorkspaceId,
    /// Canonical registry name, never an alias.
    pub canonical_name: String,
    /// Stable external ingress identity from the workspace manifest.
    pub ingress_id: IngressId,
    /// Operating-system identity of the live TUI.
    pub tui_pid: u32,
    /// TUI-resolved root used only to compare with current machine state.
    pub resolved_root: PathBuf,
    /// UUID-scoped socket on which the TUI accepts jobs.
    pub job_socket: PathBuf,
}

/// One newline-delimited control request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ControlRequest {
    /// Register a newly ready TUI.
    Register(LeaseRegistration),
    /// Renew one live lease.
    Heartbeat {
        /// Target process generation.
        generation: ServerGeneration,
        /// Lease to renew.
        lease_id: LeaseId,
    },
    /// Reload persistent receiver intent for one exact workspace.
    RefreshEnabled {
        /// Target process generation.
        generation: ServerGeneration,
        /// Stable workspace whose live lease should be refreshed.
        workspace_id: WorkspaceId,
    },
    /// Remove one live lease before its TUI tears down.
    Unregister {
        /// Target process generation.
        generation: ServerGeneration,
        /// Lease to remove.
        lease_id: LeaseId,
    },
    /// Resolve the accepted ingress of one live workspace registration.
    WorkspaceIngress {
        /// Target process generation.
        generation: ServerGeneration,
        /// Exact workspace whose accepted ingress is requested.
        workspace_id: WorkspaceId,
    },
    /// Inspect receiver enablement on one exact live workspace lease.
    WorkspaceStatus {
        /// Target process generation.
        generation: ServerGeneration,
        /// Exact workspace whose live lease is requested.
        workspace_id: WorkspaceId,
    },
    /// Return non-sensitive process and lease status.
    Snapshot,
}

impl ControlRequest {
    pub(super) const fn generation(&self) -> Option<ServerGeneration> {
        match self {
            Self::Register(registration) => Some(registration.generation),
            Self::Heartbeat { generation, .. }
            | Self::RefreshEnabled { generation, .. }
            | Self::Unregister { generation, .. }
            | Self::WorkspaceIngress { generation, .. }
            | Self::WorkspaceStatus { generation, .. } => Some(*generation),
            Self::Snapshot => None,
        }
    }
}

/// Non-sensitive shared-server status returned to local clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSnapshot {
    /// Current process generation.
    pub generation: ServerGeneration,
    /// Number of unexpired live TUI leases.
    pub live_leases: usize,
}

/// One newline-delimited control response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ControlResponse {
    /// The mutation was accepted.
    Accepted {
        /// Current process generation.
        generation: ServerGeneration,
        /// Whether the final unregister requests process shutdown.
        shutdown: bool,
    },
    /// Current non-sensitive server status.
    Snapshot(ServerSnapshot),
    /// The accepted ingress of an exact live workspace registration.
    WorkspaceIngress {
        /// Current process generation.
        generation: ServerGeneration,
        /// `None` when that workspace has no live lease in this generation.
        ingress_id: Option<IngressId>,
        /// Ephemeral local capability owned by that exact live lease.
        lease_id: Option<LeaseId>,
    },
    /// Receiver enablement snapshot of an exact live workspace lease.
    WorkspaceStatus {
        /// Current process generation.
        generation: ServerGeneration,
        /// Number of unexpired leases in this generation.
        live_leases: usize,
        /// `None` when that workspace has no live lease in this generation.
        receiver_enabled: Option<bool>,
    },
    /// The request targeted a previous process generation.
    StaleGeneration,
    /// The request was syntactically valid but could not be accepted.
    Rejected {
        /// Human-readable local diagnostic.
        message: String,
    },
}
