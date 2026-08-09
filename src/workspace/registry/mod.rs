//! The versioned, machine-global workspace registry.

mod lock;
mod migrate;
mod model;
mod select;
mod store;
mod upgrade;
mod validate;

pub(crate) use migrate::migrate_legacy_with;
pub use migrate::{MigrationOutcome, migrate_legacy};
pub use model::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, ReceiverAction, WorkspaceRecord, receiver_transition,
};
pub use select::SelectedWorkspace;
pub use store::RegistryStore;
pub use validate::{RegistryError, RegistryOperation, validate_registry};

#[cfg(test)]
mod tests;
