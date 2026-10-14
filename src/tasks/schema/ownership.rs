//! Acquiring and caller-owned task-schema migration boundaries.

use super::{MigrationOutcome, MigrationStep, TaskSchemaMigration, migrate_owned_with_hook};
use anyhow::{Result, bail};

/// Apply the schema conversion only after a rollout coordinator supplies the
/// legacy-sync decision and a machine-local backup destination.
///
/// This inactive primitive only has a lock path; native coordinators use the recovering
/// workspace-owned entry point.
pub fn migrate_inactive(request: TaskSchemaMigration<'_>) -> Result<MigrationOutcome> {
    migrate_inactive_with_hook(request, |_| Ok(()))
}

pub(super) fn migrate_inactive_with_hook(
    request: TaskSchemaMigration<'_>,
    hook: impl FnMut(MigrationStep) -> std::io::Result<()>,
) -> Result<MigrationOutcome> {
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire_path(request.task_store_lock)?;
    migrate_owned_with_hook(request, &owner, hook)
}

pub(crate) fn migrate_inactive_owned(
    workspace: &crate::workspace::WorkspaceContext,
    owner: &crate::tasks::store_lock::TaskStoreOwner,
    request: TaskSchemaMigration<'_>,
) -> Result<MigrationOutcome> {
    owner.recover(workspace)?;
    if request.workspace_root != workspace.root() || request.workspace_id != workspace.id() {
        bail!("task schema migration belongs to a different workspace");
    }
    migrate_owned_with_hook(request, owner, |_| Ok(()))
}
