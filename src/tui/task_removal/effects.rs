//! Shared safe consent projection; no private snapshots cross the public surface.
use crate::tasks::removal::{model::RemovalPlan, presentation};
pub(super) fn relationships(plan: &RemovalPlan) -> Vec<String> {
    presentation::project(plan).lines()
}
