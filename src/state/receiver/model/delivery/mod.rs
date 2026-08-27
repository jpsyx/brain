//! Immutable receiver response delivery types.

mod envelope;
mod identity;
mod status;

pub use envelope::{
    ReceiverDeliveryEnvelope, ReceiverDeliveryRenderError, ReceiverEmailEnvelope,
    ReceiverSmsEnvelope, render_receiver_delivery,
};
pub use identity::{
    ReceiverDeliveryAttemptId, ReceiverDeliveryId, ReceiverProviderReference, ReceiverResponseKind,
};
pub use status::{
    ReceiverDeliveryAmbiguity, ReceiverDeliveryErrorCategory, ReceiverDeliveryRetryMetadata,
    ReceiverDeliveryState, ReceiverDeliveryStatus,
};
