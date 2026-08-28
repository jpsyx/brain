//! Durable receiver data types grouped by lifecycle responsibility.

mod answer_cleanup;
mod claim;
mod conversation;
mod delivery;
mod effect;
mod identity;
mod job;
mod observation;

pub use answer_cleanup::ReceiverAnswerCleanup;
pub use claim::{ReceiverAcceptance, ReceiverClaim, ReceiverRunClaim};
pub use conversation::{
    ReceiverConversation, ReceiverSessionBinding, ReceiverSessionBindingError, ReceiverSessionPlan,
};
pub use delivery::{
    ReceiverDeliveryAmbiguity, ReceiverDeliveryApplyOutcome, ReceiverDeliveryAttemptId,
    ReceiverDeliveryClaim, ReceiverDeliveryEnvelope, ReceiverDeliveryErrorCategory,
    ReceiverDeliveryId, ReceiverDeliveryRenderError, ReceiverDeliveryRetryMetadata,
    ReceiverDeliveryState, ReceiverDeliveryStatus, ReceiverEmailEnvelope,
    ReceiverProviderReference, ReceiverResponseKind, ReceiverSmsEnvelope, render_receiver_delivery,
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
    ReceiverCompletionOutcome, ReceiverCompletionRequest, ReceiverLaunchObservation,
    ReceiverNonterminalObservationPhase, ReceiverObservation, ReceiverObservationSet,
};
