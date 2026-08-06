//! Journaled rollout from legacy task identity to multi-workspace identity.

mod backup;
mod coordinator;
mod journal;
mod plan;
mod steps;
mod users;
mod verify;

pub use backup::{backup_directory, backup_portable_data};
pub(crate) use coordinator::run;
pub use journal::{JournalRequest, MigrationJournal};
pub use plan::{MigrationState, PlanInput, Step, discover_state, migration_plan};
pub use steps::{
    MappingIssue, MappingResolution, MigrationGate, MigrationGateInput, apply_mapping_resolution,
    headless_mapping_remediation, mapping_issues, migration_gate,
};
