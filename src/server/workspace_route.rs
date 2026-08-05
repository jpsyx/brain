//! Resolve one opaque public ingress through a live lease before loading any
//! workspace-specific state.

use std::fmt::{Display, Formatter};
use std::path::Path;
use std::time::Instant;

use crate::server::lifecycle::{
    AuthorityRevision, IngressId, LeaseTable, ServerGeneration, WorkspaceAvailability,
    WorkspaceLease,
};
use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceManifest};

/// One route after live availability and authoritative workspace identity
/// have both been verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspaceRoute {
    context: WorkspaceContext,
    lease: WorkspaceLease,
    registry_store: RegistryStore,
    authority_ticket: Option<WorkspaceRouteTicket>,
}

impl ResolvedWorkspaceRoute {
    pub(crate) const fn new(
        context: WorkspaceContext,
        lease: WorkspaceLease,
        registry_store: RegistryStore,
    ) -> Self {
        Self {
            context,
            lease,
            registry_store,
            authority_ticket: None,
        }
    }

    pub(crate) const fn with_authority(
        context: WorkspaceContext,
        lease: WorkspaceLease,
        registry_store: RegistryStore,
        authority_ticket: WorkspaceRouteTicket,
    ) -> Self {
        Self {
            context,
            lease,
            registry_store,
            authority_ticket: Some(authority_ticket),
        }
    }

    /// The freshly reloaded, verified workspace context.
    #[must_use]
    pub const fn context(&self) -> &WorkspaceContext {
        &self.context
    }

    /// The live lease that authorized this route.
    #[must_use]
    pub const fn lease(&self) -> &WorkspaceLease {
        &self.lease
    }

    /// The exact machine registry capability used to verify this route.
    #[must_use]
    pub const fn registry_store(&self) -> &RegistryStore {
        &self.registry_store
    }

    pub(crate) fn authority_ticket(&self) -> Result<&WorkspaceRouteTicket, WorkspaceRouteError> {
        self.authority_ticket.as_ref().ok_or_else(|| {
            WorkspaceRouteError::new(503, "workspace route has no shared-server authority")
        })
    }

    pub(crate) fn revalidate_receiver_intent(&self) -> Result<(), WorkspaceRouteError> {
        let registry = RegistryStore::load_from(self.registry_store.path()).map_err(|error| {
            WorkspaceRouteError::new(500, format!("workspace registry unavailable: {error}"))
        })?;
        let record = registry
            .workspaces
            .get(&self.lease.canonical_name)
            .ok_or_else(|| WorkspaceRouteError::new(404, "workspace is no longer attached"))?;
        if record.workspace_id != self.lease.workspace_id {
            return Err(WorkspaceRouteError::new(
                409,
                "workspace registry identity does not match its live lease",
            ));
        }
        if !record.receiver_enabled {
            return Err(WorkspaceRouteError::new(
                503,
                "workspace receiver route is disabled",
            ));
        }
        Ok(())
    }
}

/// Exact live authority captured before workspace-specific state is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceRouteTicket {
    generation: ServerGeneration,
    authority_revision: AuthorityRevision,
    lease: WorkspaceLease,
}

impl WorkspaceRouteTicket {
    /// The selected lease is the sole input to workspace-specific loading.
    pub(crate) const fn lease(&self) -> &WorkspaceLease {
        &self.lease
    }
}

/// Load a workspace context selected only by a captured live lease.
pub(crate) trait WorkspaceContextLoader: Send {
    fn load(&self, lease: &WorkspaceLease) -> Result<WorkspaceContext, WorkspaceRouteError>;
}

/// Filesystem loader bound to the machine registry and runtime home.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedWorkspaceContextLoader {
    registry_store: RegistryStore,
    runtime_home: std::path::PathBuf,
}

impl VerifiedWorkspaceContextLoader {
    pub(crate) const fn new(
        registry_store: RegistryStore,
        runtime_home: std::path::PathBuf,
    ) -> Self {
        Self {
            registry_store,
            runtime_home,
        }
    }
}

impl WorkspaceContextLoader for VerifiedWorkspaceContextLoader {
    fn load(&self, lease: &WorkspaceLease) -> Result<WorkspaceContext, WorkspaceRouteError> {
        load_verified_context(&self.registry_store, &self.runtime_home, lease)
    }
}

/// Ingress resolver retained for direct, single-owner callers.
pub struct WorkspaceRouteResolver<'a> {
    leases: &'a mut LeaseTable,
    registry_store: &'a RegistryStore,
    runtime_home: &'a Path,
    now: Instant,
}

impl<'a> WorkspaceRouteResolver<'a> {
    /// Build a resolver over one process's current lease table.
    #[must_use]
    pub const fn new(
        leases: &'a mut LeaseTable,
        registry_store: &'a RegistryStore,
        runtime_home: &'a Path,
        now: Instant,
    ) -> Self {
        Self {
            leases,
            registry_store,
            runtime_home,
            now,
        }
    }

    /// Resolve an ingress through one currently accepting lease.
    ///
    /// # Errors
    ///
    /// Returns an error when the ingress is unavailable or its workspace
    /// registry and manifest identity cannot be verified.
    pub fn resolve(
        &mut self,
        ingress: IngressId,
    ) -> Result<ResolvedWorkspaceRoute, WorkspaceRouteError> {
        let lease = accepting_lease(self.leases, ingress, self.now)?;
        let context = load_verified_context(self.registry_store, self.runtime_home, &lease)?;
        Ok(ResolvedWorkspaceRoute::new(
            context,
            lease,
            self.registry_store.clone(),
        ))
    }
}

/// Pure lease-table half of the shared process's two-phase route protocol.
pub(crate) struct WorkspaceRouteAuthority;

impl WorkspaceRouteAuthority {
    /// Capture one live accepting lease without performing filesystem IO.
    pub(crate) fn begin(
        leases: &mut LeaseTable,
        generation: ServerGeneration,
        ingress: IngressId,
        now: Instant,
    ) -> Result<WorkspaceRouteTicket, WorkspaceRouteError> {
        let lease = accepting_lease(leases, ingress, now)?;
        let authority_revision = leases
            .authority_revision(lease.workspace_id)
            .ok_or_else(|| WorkspaceRouteError::new(503, "workspace route authority is stale"))?;
        Ok(WorkspaceRouteTicket {
            generation,
            authority_revision,
            lease,
        })
    }

    pub(crate) fn begin_local(
        leases: &mut LeaseTable,
        generation: ServerGeneration,
        ingress: IngressId,
        capability: crate::server::lifecycle::LeaseId,
        now: Instant,
    ) -> Result<WorkspaceRouteTicket, WorkspaceRouteError> {
        let ticket = Self::begin(leases, generation, ingress, now)?;
        if ticket.lease.lease_id != capability {
            return Err(WorkspaceRouteError::new(404, "local route not found"));
        }
        Ok(ticket)
    }

    /// Revalidate the same process generation and lease authority after IO.
    pub(crate) fn finish(
        leases: &mut LeaseTable,
        generation: ServerGeneration,
        ticket: &WorkspaceRouteTicket,
        now: Instant,
    ) -> Result<(), WorkspaceRouteError> {
        if ticket.generation != generation {
            return Err(WorkspaceRouteError::new(
                503,
                "workspace route authority is stale",
            ));
        }
        let current = accepting_lease(leases, ticket.lease.ingress_id, now)?;
        let current_revision = leases.authority_revision(current.workspace_id);
        if current_revision != Some(ticket.authority_revision)
            || !same_authority(&current, &ticket.lease)
        {
            return Err(WorkspaceRouteError::new(
                503,
                "workspace route authority changed while loading",
            ));
        }
        Ok(())
    }
}

fn accepting_lease(
    leases: &mut LeaseTable,
    ingress: IngressId,
    now: Instant,
) -> Result<WorkspaceLease, WorkspaceRouteError> {
    match leases.availability(ingress, now) {
        WorkspaceAvailability::Accepting(lease) => Ok(lease),
        WorkspaceAvailability::Disabled | WorkspaceAvailability::NoLiveTui => Err(
            WorkspaceRouteError::new(503, "workspace receiver route is unavailable"),
        ),
        WorkspaceAvailability::Unknown => {
            Err(WorkspaceRouteError::new(404, "workspace route not found"))
        }
    }
}

fn same_authority(current: &WorkspaceLease, ticket: &WorkspaceLease) -> bool {
    current.lease_id == ticket.lease_id
        && current.workspace_id == ticket.workspace_id
        && current.canonical_name == ticket.canonical_name
        && current.ingress_id == ticket.ingress_id
        && current.tui_pid == ticket.tui_pid
        && current.job_socket == ticket.job_socket
}

fn load_verified_context(
    registry_store: &RegistryStore,
    runtime_home: &Path,
    lease: &WorkspaceLease,
) -> Result<WorkspaceContext, WorkspaceRouteError> {
    let registry = RegistryStore::load_from(registry_store.path()).map_err(|error| {
        WorkspaceRouteError::new(500, format!("workspace registry unavailable: {error}"))
    })?;
    let record = registry
        .workspaces
        .get(&lease.canonical_name)
        .ok_or_else(|| WorkspaceRouteError::new(404, "workspace is no longer attached"))?;
    if record.workspace_id != lease.workspace_id {
        return Err(WorkspaceRouteError::new(
            409,
            "workspace registry identity does not match its live lease",
        ));
    }
    if !record.receiver_enabled {
        return Err(WorkspaceRouteError::new(
            503,
            "workspace receiver route is disabled",
        ));
    }
    if !record.root.is_dir() {
        return Err(WorkspaceRouteError::new(
            503,
            format!("workspace root {} is unavailable", record.root.display()),
        ));
    }
    let manifest =
        WorkspaceManifest::load(&record.root, env!("CARGO_PKG_VERSION")).map_err(|error| {
            WorkspaceRouteError::new(409, format!("workspace manifest is invalid: {error}"))
        })?;
    if manifest.workspace_id() != lease.workspace_id
        || IngressId::from(manifest.receiver_ingress_id()) != lease.ingress_id
    {
        return Err(WorkspaceRouteError::new(
            409,
            "workspace manifest identity does not match its live lease",
        ));
    }
    let context = WorkspaceContext::new(
        runtime_home,
        record.workspace_id,
        lease.canonical_name.clone(),
        &record.root,
        record.local_user_id.clone(),
        Path::new("/"),
    )
    .map_err(|error| {
        WorkspaceRouteError::new(409, format!("workspace root is invalid: {error}"))
    })?;
    Ok(context)
}

/// A public route could not be mapped safely to one live workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRouteError {
    status: u16,
    message: String,
}

impl WorkspaceRouteError {
    pub(crate) fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// HTTP status appropriate for the routing failure.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }
}

impl Display for WorkspaceRouteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WorkspaceRouteError {}
