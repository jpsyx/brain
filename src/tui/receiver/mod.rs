//! Live-TUI receiver state with representation-owned queue behavior.

mod decision;
mod effect;
#[allow(dead_code)]
mod failure;
#[allow(dead_code)]
pub(crate) mod planning;
pub(crate) mod policy;
mod queue;
mod runtime;
#[allow(dead_code)]
mod session;

#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod planning_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod session_tests;

pub(crate) use decision::{
    ReceiverDecision, ReceiverTickContext, ReceiverTickControl, TickStage, control_after_effect,
    run_receiver_tick,
};
pub(crate) use effect::{ReceiverEffect, ReceiverEffectOutcome};
#[allow(unused_imports)]
pub(crate) use failure::rollback_receiver_launch;
pub use queue::{InboundQueue, StageError, StagedAdmission};
pub(crate) use runtime::{
    DeliveryTarget, ReceiverProbe, ReceiverRuntime, RemoteCompletionTarget, SyncGateObservation,
    SyncGatePoll,
};
#[allow(unused_imports)]
pub(crate) use session::{ReceiverRemoteSession, ReceiverSessionRegistration};
