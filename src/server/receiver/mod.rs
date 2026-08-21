//! Authenticated inbound jobs and the bounded live-TUI forwarding boundary.

mod attachments;
mod control;
pub(crate) mod dispatch;
pub(crate) mod http;
mod job;
pub(crate) mod routing;
mod transport;
mod unavailable;

pub use attachments::stage_attachments;
pub use control::{ControlCommand, RestartPlan, parse as parse_control_command};
pub use dispatch::{DispatchPipeline, execute_pipeline, forward_job};
pub use job::{AttachmentRef, Channel, EmailReplyContext, InboundJob};
pub(crate) use unavailable::message as unavailable_message;
pub use unavailable::{ForwardOutcome, UnavailableResponse, forward_or_unavailable};

pub(crate) mod admission;
