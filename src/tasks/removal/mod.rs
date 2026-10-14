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
mod external;
pub(crate) use execute::{apply, preview};

mod lifecycle;
pub(crate) mod operations;
pub(crate) use lifecycle::{abandon, operation_status, resume};
