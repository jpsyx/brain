//! Durable receiver data types grouped by lifecycle responsibility.

mod claim;
mod conversation;
mod delivery;
mod effect;
mod identity;
mod job;
mod observation;

pub use claim::{ReceiverAcceptance, ReceiverClaim, ReceiverRunClaim};
pub use conversation::{
    ReceiverConversation, ReceiverSessionBinding, ReceiverSessionBindingError, ReceiverSessionPlan,
};
pub use delivery::{
    ReceiverDeliveryAmbiguity, ReceiverDeliveryAttemptId, ReceiverDeliveryEnvelope,
    ReceiverDeliveryErrorCategory, ReceiverDeliveryId, ReceiverDeliveryRenderError,
    ReceiverDeliveryRetryMetadata, ReceiverDeliveryState, ReceiverDeliveryStatus,
    ReceiverEmailEnvelope, ReceiverProviderReference, ReceiverResponseKind, ReceiverSmsEnvelope,
    render_receiver_delivery,
};
pub use effect::{
    MAX_RECEIVER_LAUNCH_ATTEMPTS, ReceiverLaunchFailure, ReceiverLaunchRetryOutcome,
    ReceiverReconciliationAction, ReceiverReconciliationEffect, ReceiverReconciliationReason,
    ReceiverRecoveryCleanupOutcome, ReceiverRecoveryFailure, ReceiverUnavailableNoticeClaim,
};
pub use identity::{
    ReceiverConversationId, ReceiverJobId, ReceiverJobToken, ReceiverSessionAttribution,
};
pub use job::ReceiverJob;
pub(super) use job::{
    ReceiverObservationMetadata, ReceiverRecoveryMetadata, ReceiverRetryMetadata,
    ReceiverStoredMetadata,
};
pub use observation::{
    ReceiverCompletionRequest, ReceiverLaunchObservation, ReceiverNonterminalObservationPhase,
    ReceiverObservation, ReceiverObservationSet,
};
