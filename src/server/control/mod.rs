//! Bounded, generation-tagged control protocol for the shared server.

mod client;
pub mod codec;
mod connect;
mod heartbeat;
mod protocol;
mod server;

pub use client::{RegistrationGate, ServerClient};
pub use heartbeat::{
    heartbeat_disposition, HeartbeatClock, HeartbeatDisposition, HeartbeatEvent, HeartbeatWorker,
};
pub use protocol::{ControlRequest, ControlResponse, LeaseRegistration, ServerSnapshot};
pub use server::{ControlListener, ControlServer};
