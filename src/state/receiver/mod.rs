//! Durable receiver job and logical-conversation state.

mod identity;
mod job_state;
mod model;
mod recovery_policy;
pub(crate) mod schema;
mod store;

#[cfg(test)]
mod tests;

pub use identity::{EmailLineage, EmailLineageError, ReceiverConversationIdentity};
pub use job_state::ReceiverJobState;
pub use model::{
    MAX_RECEIVER_LAUNCH_ATTEMPTS, ReceiverAcceptance, ReceiverClaim, ReceiverCompletionRequest,
    ReceiverConversation, ReceiverConversationId, ReceiverJob, ReceiverJobId, ReceiverJobToken,
    ReceiverLaunchFailure, ReceiverLaunchObservation, ReceiverLaunchRetryOutcome,
    ReceiverNonterminalObservationPhase, ReceiverObservation, ReceiverObservationSet,
    ReceiverRunClaim, ReceiverSessionAttribution, ReceiverSessionBinding,
    ReceiverSessionBindingError, ReceiverSessionPlan,
};
use model::{
    ReceiverObservationMetadata, ReceiverRecoveryMetadata, ReceiverRetryMetadata,
    ReceiverStoredMetadata,
};
pub use recovery_policy::{
    MAX_RECEIVER_RECOVERY_ATTEMPTS, ReceiverAttemptKind, ReceiverLifecycleDeadlines,
    ReceiverRecoveryDecision, ReceiverRecoverySnapshot, decide_receiver_recovery,
    receiver_acceptance_expires_at, receiver_launch_expires_at, receiver_recovery_expires_at,
};
