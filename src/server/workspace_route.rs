//! Resolve one opaque public ingress through a live lease before loading any
//! workspace-specific state.

use std::fmt::{Display, Formatter};
use std::path::Path;
use std::time::Instant;

use crate::server::lifecycle::{IngressId, LeaseTable, WorkspaceAvailability, WorkspaceLease};
use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceManifest};

/// One route after live availability and authoritative workspace identity
/// have both been verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspaceRoute {
    context: WorkspaceContext,
    lease: WorkspaceLease,
}

impl ResolvedWorkspaceRoute {
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
}

/// Ingress resolver whose construction order makes the live lease the only
/// selector for later workspace state.
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

    /// Resolve `ingress` to a live lease, then reopen and verify its exact
    /// registry record and portable manifest.
    ///
    /// # Errors
    ///
    /// Returns 404 semantics for an ingress this process has never observed,
    /// 503 when its known lease is no longer live, and refuses inconsistent
    /// registry or manifest identity without returning a context.
    pub fn resolve(
        &mut self,
        ingress: IngressId,
    ) -> Result<ResolvedWorkspaceRoute, WorkspaceRouteError> {
        let lease = match self.leases.availability(ingress, self.now) {
            WorkspaceAvailability::Accepting(lease) => lease,
            WorkspaceAvailability::Disabled | WorkspaceAvailability::NoLiveTui => {
                return Err(WorkspaceRouteError::new(
                    503,
                    "workspace receiver route is unavailable",
                ));
            }
            WorkspaceAvailability::Unknown => {
                return Err(WorkspaceRouteError::new(404, "workspace route not found"));
            }
        };

        let registry = RegistryStore::load_from(self.registry_store.path()).map_err(|error| {
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
            self.runtime_home,
            record.workspace_id,
            lease.canonical_name.clone(),
            &record.root,
            record.local_user_id.clone(),
            Path::new("/"),
        )
        .map_err(|error| {
            WorkspaceRouteError::new(409, format!("workspace root is invalid: {error}"))
        })?;
        Ok(ResolvedWorkspaceRoute { context, lease })
    }
}

/// A public route could not be mapped safely to one live workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRouteError {
    status: u16,
    message: String,
}

impl WorkspaceRouteError {
    fn new(status: u16, message: impl Into<String>) -> Self {
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
