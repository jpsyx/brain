//! Live-TUI receiver state with representation-owned queue behavior.

mod failure;
pub(crate) mod planning;
pub(crate) mod policy;
mod queue;
mod run;
#[allow(dead_code)]
mod runtime;
mod session;

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod planning_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod test_support;

pub(crate) use failure::rollback_receiver_launch;
pub use queue::{InboundQueue, StageError, StagedAdmission};
pub(crate) use run::{
    ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun, ReceiverEffectOutcome,
};
pub(crate) use runtime::{ReceiverRuntime, SyncGateObservation, SyncGatePoll};
pub(crate) use session::{
    ReceiverRemoteSession, ReceiverSessionRegistration, ReceiverSessionStore,
};
