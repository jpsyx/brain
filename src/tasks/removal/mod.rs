//! Native removal policy and snapshot target resolution.

pub(crate) mod dependencies;
pub(crate) mod external_policy;
pub(crate) mod fingerprint;
pub(crate) mod integration;
pub(crate) mod model;
pub(crate) mod plan;
pub(crate) mod projects;
pub(crate) mod references;
pub(crate) mod resolve;
pub(crate) mod snapshot;

#[cfg(test)]
mod tests;

mod execute;
pub(crate) use execute::{apply, preview};

#[allow(dead_code)] // Task12 wires the operation lifecycle command grammar.
mod lifecycle;
pub(crate) mod operations;
#[allow(unused_imports)] // Command grammar in Task12 consumes the lifecycle surface.
pub(crate) use lifecycle::{abandon, operation_status, resume};
