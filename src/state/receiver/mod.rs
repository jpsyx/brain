//! Durable receiver job and logical-conversation state.

mod delivery_policy;
mod identity;
mod job_state;
mod model;
mod recovery_policy;
pub(crate) mod schema;
mod store;

#[cfg(test)]
mod tests;

pub use delivery_policy::{
    ReceiverDeliveryDecision, ReceiverDeliveryPolicySnapshot, ReceiverProviderCapability,
    ReceiverProviderResultClass, decide_receiver_delivery, receiver_delivery_retry_is_due,
};
pub use identity::{EmailLineage, EmailLineageError, ReceiverConversationIdentity};
pub use job_state::ReceiverJobState;
pub use model::{
    MAX_RECEIVER_LAUNCH_ATTEMPTS, ReceiverAcceptance, ReceiverClaim, ReceiverCompletionRequest,
    ReceiverConversation, ReceiverConversationId, ReceiverDeliveryAmbiguity,
    ReceiverDeliveryAttemptId, ReceiverDeliveryEnvelope, ReceiverDeliveryErrorCategory,
    ReceiverDeliveryId, ReceiverDeliveryRenderError, ReceiverDeliveryRetryMetadata,
    ReceiverDeliveryState, ReceiverDeliveryStatus, ReceiverEmailEnvelope, ReceiverJob,
    ReceiverJobId, ReceiverJobToken, ReceiverLaunchFailure, ReceiverLaunchObservation,
    ReceiverLaunchRetryOutcome, ReceiverNonterminalObservationPhase, ReceiverObservation,
    ReceiverObservationSet, ReceiverProviderReference, ReceiverReconciliationAction,
    ReceiverReconciliationEffect, ReceiverReconciliationReason, ReceiverRecoveryCleanupOutcome,
    ReceiverRecoveryFailure, ReceiverResponseKind, ReceiverRunClaim, ReceiverSessionAttribution,
    ReceiverSessionBinding, ReceiverSessionBindingError, ReceiverSessionPlan, ReceiverSmsEnvelope,
    ReceiverUnavailableNoticeClaim, render_receiver_delivery,
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
