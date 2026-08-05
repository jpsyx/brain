//! Typed lease, ingress, and timing values for the shared server.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::workspace::{WorkspaceId, WorkspaceName};

/// How often a live TUI sends a heartbeat to the shared process.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

/// How long a shared process retains a lease without a heartbeat.
pub const LEASE_TTL: Duration = Duration::from_secs(5);

/// The heartbeat schedule used by one server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseTiming {
    heartbeat_interval: Duration,
    ttl: Duration,
}

impl LeaseTiming {
    /// Production heartbeat schedule. Tests inject their own values instead.
    pub const PRODUCTION: Self = Self::new(HEARTBEAT_INTERVAL, LEASE_TTL);

    /// Build a heartbeat schedule.
    #[must_use]
    pub const fn new(heartbeat_interval: Duration, ttl: Duration) -> Self {
        Self {
            heartbeat_interval,
            ttl,
        }
    }

    /// The interval between heartbeats.
    #[must_use]
    pub const fn heartbeat_interval(self) -> Duration {
        self.heartbeat_interval
    }

    /// The maximum age of an unrenewed lease.
    #[must_use]
    pub const fn ttl(self) -> Duration {
        self.ttl
    }
}

impl Default for LeaseTiming {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

/// Opaque identity for one TUI registration attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeaseId(Uuid);

impl LeaseId {
    /// Create a fresh lease identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a lease identity from a UUID string.
    ///
    /// # Errors
    ///
    /// Returns [`LeaseIdError`] when `value` is not a UUID.
    pub fn parse(value: &str) -> Result<Self, LeaseIdError> {
        Uuid::parse_str(value).map(Self).map_err(|_| LeaseIdError)
    }
}

impl Default for LeaseId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for LeaseId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for LeaseId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LeaseId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// An invalid lease UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseIdError;

impl Display for LeaseIdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("lease ID must be a UUID")
    }
}

impl Error for LeaseIdError {}

/// Stable opaque public route identity for one workspace receiver.
///
/// This intentionally remains distinct from [`WorkspaceId`] even though both
/// serialize as UUID strings. Existing manifests retain their UUID bytes, and
/// callers convert at the manifest boundary instead of treating an ingress as
/// a workspace selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IngressId(Uuid);

impl IngressId {
    /// Create a fresh ingress identity for a newly initialized workspace.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse an ingress identity from a UUID string.
    ///
    /// # Errors
    ///
    /// Returns [`IngressIdError`] when `value` is not a UUID.
    pub fn parse(value: &str) -> Result<Self, IngressIdError> {
        Uuid::parse_str(value).map(Self).map_err(|_| IngressIdError)
    }
}

impl Default for IngressId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<WorkspaceId> for IngressId {
    fn from(workspace_id: WorkspaceId) -> Self {
        Self::parse(&workspace_id.to_string()).expect("workspace IDs always contain UUIDs")
    }
}

impl From<IngressId> for WorkspaceId {
    fn from(ingress_id: IngressId) -> Self {
        Self::parse(&ingress_id.to_string()).expect("ingress IDs always contain UUIDs")
    }
}

impl Display for IngressId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for IngressId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for IngressId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// An invalid ingress UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngressIdError;

impl Display for IngressIdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ingress ID must be a UUID")
    }
}

impl Error for IngressIdError {}

/// The live registration one TUI gives the shared process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLease {
    /// Opaque registration identity, regenerated for every TUI attachment.
    pub lease_id: LeaseId,
    /// Stable machine registry identity for the workspace.
    pub workspace_id: WorkspaceId,
    /// Canonical registry key captured at registration time.
    pub canonical_name: WorkspaceName,
    /// Opaque ingress identity used to choose this workspace before loading it.
    pub ingress_id: IngressId,
    /// PID of the registered TUI, used only by later process integration.
    pub tui_pid: u32,
    /// Workspace-local socket that receives acknowledged jobs.
    pub job_socket: PathBuf,
    /// Receiver intent captured with this live TUI registration.
    pub receiver_enabled: bool,
    /// Monotonic deadline after which this lease is stale.
    pub expires_at: Instant,
}

/// Routing availability for one ingress at one monotonic instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAvailability {
    /// A live receiver-enabled TUI owns this ingress.
    Accepting(WorkspaceLease),
    /// A live TUI owns the ingress but has disabled the receiver.
    Disabled,
    /// The ingress was registered before, but no live TUI currently owns it.
    NoLiveTui,
    /// This process has never observed the ingress.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::IngressId;
    use crate::workspace::WorkspaceId;

    const UUID: &str = "57b162df-983a-45c3-ac7e-bad94eb27a99";

    #[test]
    fn ingress_id_preserves_the_manifest_uuid_string_representation() {
        let workspace_id = WorkspaceId::parse(UUID).expect("valid manifest UUID");
        let ingress_id = IngressId::from(workspace_id);

        assert_eq!(
            serde_json::to_string(&ingress_id).unwrap(),
            format!("\"{UUID}\"")
        );
        assert_eq!(WorkspaceId::from(ingress_id), workspace_id);
    }
}
