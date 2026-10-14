//! Native removal policy and snapshot target resolution.

// The planner and command surfaces consume this module in subsequent slices.
#![allow(dead_code)]

pub(crate) mod fingerprint;
pub(crate) mod model;
pub(crate) mod resolve;
pub(crate) mod snapshot;

#[cfg(test)]
mod tests;
