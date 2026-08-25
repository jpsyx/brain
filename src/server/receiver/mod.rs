//! Authenticated inbound jobs and the durable admission and dispatch boundary.

mod attachments;
mod control;
pub(crate) mod dispatch;
pub(crate) mod http;
mod job;
pub(crate) mod routing;
mod unavailable;

pub use attachments::{
    MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_COUNT, StagedAttachment, stage_attachments,
};
pub use control::{ControlCommand, RestartPlan, parse as parse_control_command};
pub use dispatch::{DispatchPipeline, execute_pipeline};
pub use job::{AttachmentRef, Channel, EmailReplyContext, InboundJob};
pub(crate) use unavailable::message as unavailable_message;

pub(crate) mod admission;
