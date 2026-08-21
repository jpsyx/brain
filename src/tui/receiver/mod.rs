//! Live-TUI receiver state with representation-owned queue behavior.

mod decision;
mod effect;
mod queue;
mod runtime;

#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod runtime_tests;

pub(crate) use decision::{
    ReceiverDecision, ReceiverTickContext, ReceiverTickControl, TickStage, control_after_effect,
    run_receiver_tick,
};
pub(crate) use effect::{ReceiverEffect, ReceiverEffectOutcome};
pub use queue::{InboundQueue, StageError, StagedAdmission};
pub(crate) use runtime::{
    DeliveryTarget, ReceiverProbe, ReceiverRuntime, RemoteCompletionTarget, SyncGateObservation,
    SyncGatePoll,
};
