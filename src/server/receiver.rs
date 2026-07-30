//! TUI-owned external message receiver.
//!
//! The listener is intentionally a child of the interactive brain process.
//! Dropping [`ReceiverServer`] closes the socket, so it cannot become a
//! detached service on machines that are not meant to receive messages.

mod attachments;
mod control;
mod http;

pub use attachments::stage_attachments;
pub use control::{ControlSocket, send_control};
pub use http::ReceiverServer;

pub const DEFAULT_PORT: u16 = 8788;
pub const INBOUND_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Sms,
    Email,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub channel: Channel,
    pub body: String,
    pub sender: String,
    pub participants: Vec<String>,
    pub provider_id: Option<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub url: String,
    pub content_type: Option<String>,
    pub filename: Option<String>,
}
