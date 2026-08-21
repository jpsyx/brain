//! Live-TUI receiver state with representation-owned queue behavior.

mod decision;
mod effect;
mod queue;
mod runtime;

#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod runtime_tests;

pub(crate) use decision::{ReceiverDecision, ReceiverTickContext, TickStage};
pub(crate) use effect::ReceiverEffect;
pub use queue::{InboundQueue, StageError, StagedAdmission};
pub(crate) use runtime::{
    DeliveryTarget, ReceiverProbe, ReceiverRuntime, RemoteCompletionTarget, SyncGateObservation,
    SyncGatePoll,
};
