//! Shared launch and lifecycle for the permanent Main and named manual tabs.

mod launch;
mod lifecycle;
mod restore;

pub(in crate::tui) use launch::ManualLaunchTarget;
