//! Authenticated inbound jobs and the bounded live-TUI forwarding boundary.

mod attachments;
pub(crate) mod dispatch;
pub(crate) mod http;
mod job;
pub(crate) mod routing;
mod transport;
mod unavailable;

pub use attachments::stage_attachments;
pub use dispatch::{DispatchPipeline, execute_pipeline, forward_job};
pub use job::{AttachmentRef, Channel, EmailReplyContext, InboundJob};
pub(crate) use unavailable::message as unavailable_message;
pub use unavailable::{ForwardOutcome, UnavailableResponse, forward_or_unavailable};

pub const INBOUND_QUEUE_CAPACITY: usize = 64;
pub(crate) mod admission;
