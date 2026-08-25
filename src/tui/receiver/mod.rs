//! Durable receiver-run scheduling and session ownership.

pub(crate) mod attachments;
mod failure;
pub(crate) mod planning;
mod run;
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
pub(crate) use run::{
    ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun, ReceiverEffectOutcome,
};
pub(crate) use runtime::{ReceiverRuntime, SyncGateObservation, SyncGatePoll};
pub(crate) use session::{
    ReceiverRemoteSession, ReceiverSessionRegistration, ReceiverSessionStore,
};
