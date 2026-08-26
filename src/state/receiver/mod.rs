//! Durable receiver job and logical-conversation state.

mod identity;
mod job_state;
mod model;
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
use model::{ReceiverObservationMetadata, ReceiverRetryMetadata};
