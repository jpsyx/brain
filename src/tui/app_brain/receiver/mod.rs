//! Receiver work owned by the main brain controller.

mod active;
mod artifact;
mod attachment_dispatch;
mod cleanup;
mod control;
pub(in crate::tui::app_brain) mod diagnostic;
mod dispatch;
mod launch;
mod launch_effects;
mod notice;
mod ownership;
mod recovery;
mod recovery_launch;
mod reply;
mod resume;
mod shutdown;
