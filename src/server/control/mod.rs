//! Bounded, generation-tagged control protocol for the shared server.

mod client;
pub mod codec;
mod heartbeat;
mod protocol;
mod server;

pub use client::ServerClient;
pub use heartbeat::{HeartbeatDisposition, HeartbeatEvent, HeartbeatWorker, heartbeat_disposition};
pub use protocol::{ControlRequest, ControlResponse, LeaseRegistration, ServerSnapshot};
pub use server::{ControlListener, ControlServer};
