//! Strict configuration and bounded external issue adapter boundary.
pub(crate) mod config;
#[allow(dead_code)] // Task11 wires invocation into the persisted operation coordinator.
pub(crate) mod process;
#[allow(dead_code)] // Shared transport DTOs are consumed by Task11.
pub(crate) mod protocol;
#[cfg(test)]
mod tests;
