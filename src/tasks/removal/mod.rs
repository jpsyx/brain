//! Native removal policy and snapshot target resolution.

// The planner and command surfaces consume this module in subsequent slices.
#![allow(dead_code)]

pub(crate) mod model;
pub(crate) mod resolve;

#[cfg(test)]
mod tests;
