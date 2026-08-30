//! Bounded, generation-tagged control protocol for the shared server.

mod client;
pub mod codec;
pub(crate) mod connect;
mod heartbeat;
mod protocol;
mod server;
mod status;

pub use client::{RegistrationGate, ServerClient};
pub use heartbeat::{
    HeartbeatClock, HeartbeatDisposition, HeartbeatEvent, HeartbeatWorker, heartbeat_disposition,
};
pub use protocol::{
    CONTROL_PROTOCOL_VERSION, ControlRequest, ControlResponse, LeaseRegistration, ServerSnapshot,
};
pub use server::{ControlListener, ControlServer};
pub use status::WorkspaceStatusSnapshot;
pub(crate) use status::{ProtocolMismatch, is_protocol_mismatch};
