//! Pure migration state and ordered-step planning.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Portable task-schema state as seen by the rollout coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    Legacy,
    Prepared,
    Current,
    NewerRefused { found: u64 },
}

/// Inputs that affect the ordered rollout plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanInput {
    pub state: MigrationState,
    pub sync_configured: bool,
}

/// One journaled rollout step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    LegacySemanticSync,
    BackupPortableData,
    EnsureWorkspaceManifest,
    EnsureUsersRegistry,
    MigrateTaskColumnsAndUuids,
    ReconcileManagedTriage,
    RebuildDerivedData,
    Verify,
    MarkComplete,
}

/// Inspect portable files without changing the workspace.
pub fn discover_state(root: &Path) -> Result<MigrationState> {
    let inspection = crate::tasks::schema::inspect_inactive(root)?;
    if let Some(found) = inspection.version
        && found > crate::tasks::schema::TASK_SCHEMA_VERSION
    {
        return Ok(MigrationState::NewerRefused { found });
    }
    if inspection.current {
        return Ok(MigrationState::Current);
    }
    let prepared = crate::workspace::WorkspaceManifest::path(root).exists()
        || root.join(".config/users.json").exists()
        || inspection.version.is_some();
    Ok(if prepared {
        MigrationState::Prepared
    } else {
        MigrationState::Legacy
    })
}

/// Build the exact rollout plan for one classified workspace.
pub fn migration_plan(input: PlanInput) -> Result<Vec<Step>> {
    if input.state == MigrationState::Current {
        return Ok(Vec::new());
    }
    if let MigrationState::NewerRefused { found } = input.state {
        bail!(
            "task schema {found} is newer than this Brain supports; this Brain supports schema {}",
            crate::tasks::schema::TASK_SCHEMA_VERSION
        );
    }
    if !matches!(
        input.state,
        MigrationState::Legacy | MigrationState::Prepared
    ) {
        bail!("migration plan state is not implemented yet");
    }

    let mut steps = Vec::new();
    if input.sync_configured {
        steps.push(Step::LegacySemanticSync);
    }
    steps.extend([
        Step::BackupPortableData,
        Step::EnsureWorkspaceManifest,
        Step::EnsureUsersRegistry,
        Step::MigrateTaskColumnsAndUuids,
        Step::ReconcileManagedTriage,
        Step::RebuildDerivedData,
        Step::Verify,
        Step::MarkComplete,
    ]);
    Ok(steps)
}
