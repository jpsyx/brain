use std::path::Path;

use crate::server::lifecycle::{IngressId, WorkspaceLease};
use crate::workspace::{RegistryStore, WorkspaceContext, WorkspaceManifest};

use super::WorkspaceRouteError;

/// Load a workspace context selected only by a captured live lease.
pub(crate) trait WorkspaceContextLoader: Send {
    fn load(&self, lease: &WorkspaceLease) -> Result<WorkspaceContext, WorkspaceRouteError>;
}

/// Filesystem loader bound to the machine registry and runtime home.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedWorkspaceContextLoader {
    registry_store: RegistryStore,
    runtime_home: std::path::PathBuf,
    require_receiver_enabled: bool,
}

impl VerifiedWorkspaceContextLoader {
    pub(crate) const fn new(
        registry_store: RegistryStore,
        runtime_home: std::path::PathBuf,
    ) -> Self {
        Self {
            registry_store,
            runtime_home,
            require_receiver_enabled: true,
        }
    }

    pub(crate) const fn new_local(
        registry_store: RegistryStore,
        runtime_home: std::path::PathBuf,
    ) -> Self {
        Self {
            registry_store,
            runtime_home,
            require_receiver_enabled: false,
        }
    }
}

impl WorkspaceContextLoader for VerifiedWorkspaceContextLoader {
    fn load(&self, lease: &WorkspaceLease) -> Result<WorkspaceContext, WorkspaceRouteError> {
        load_verified_context(
            &self.registry_store,
            &self.runtime_home,
            lease,
            self.require_receiver_enabled,
        )
    }
}

pub(super) fn load_verified_context(
    registry_store: &RegistryStore,
    runtime_home: &Path,
    lease: &WorkspaceLease,
    require_receiver_enabled: bool,
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
    if require_receiver_enabled && !record.receiver_enabled {
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
