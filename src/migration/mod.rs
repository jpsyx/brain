//! Journaled rollout from legacy task identity to multi-workspace identity.

use anyhow::{Context as _, Result, bail};

mod backup;
mod coordinator;
mod journal;
mod plan;
mod schema_transition;
mod steps;
mod users;
mod verify;

pub use backup::{backup_directory, backup_portable_data};
pub(crate) use coordinator::run;
pub use journal::{JournalRequest, MigrationJournal};
pub use plan::{MigrationState, PlanInput, Step, discover_state, migration_plan};
pub use schema_transition::publish_task_schema_transition_with_transport;
pub use steps::{
    MappingIssue, MappingResolution, MigrationGate, MigrationGateInput, apply_mapping_resolution,
    headless_mapping_remediation, mapping_issues, migration_gate,
};

pub(crate) fn require_no_active_rollout(paths: &crate::workspace::WorkspacePaths) -> Result<()> {
    let journal = paths.migration_journal();
    match std::fs::metadata(&journal) {
        Ok(_) => bail!(
            "workspace migration is incomplete; resume `brain workspace migrate` before syncing"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "checking active workspace migration journal {}",
                journal.display()
            )
        }),
    }
}
