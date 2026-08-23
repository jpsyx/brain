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
    ReceiverAcceptance, ReceiverClaim, ReceiverConversation, ReceiverConversationId, ReceiverJob,
    ReceiverJobId, ReceiverSessionBinding, ReceiverSessionBindingError, ReceiverSessionPlan,
};
