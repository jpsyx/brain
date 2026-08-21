//! Live-TUI receiver state with representation-owned queue behavior.

mod queue;
mod runtime;

#[cfg(test)]
mod runtime_tests;

pub use queue::{InboundQueue, StageError, StagedAdmission};
pub(crate) use runtime::{ReceiverRuntime, SyncGatePoll};
