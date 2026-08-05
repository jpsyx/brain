//! Bounded, generation-tagged control protocol for the shared server.

mod client;
pub mod codec;
pub(crate) mod connect;
mod heartbeat;
mod protocol;
mod server;

pub use client::{RegistrationGate, ServerClient};
pub use heartbeat::{
    HeartbeatClock, HeartbeatDisposition, HeartbeatEvent, HeartbeatWorker, heartbeat_disposition,
};
pub use protocol::{ControlRequest, ControlResponse, LeaseRegistration, ServerSnapshot};
pub use server::{ControlListener, ControlServer};
