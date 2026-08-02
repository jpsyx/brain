//! The versioned, machine-global workspace registry.

mod model;
mod select;
mod store;
mod validate;

pub use model::{MachineRegistry, REGISTRY_SCHEMA_VERSION, WorkspaceRecord};
pub use select::SelectedWorkspace;
pub use store::RegistryStore;
pub use validate::{RegistryError, RegistryOperation, validate_registry};

#[cfg(test)]
mod tests;
