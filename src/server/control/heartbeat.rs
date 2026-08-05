//! TUI-owned heartbeat and crash-recovery worker.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};

use super::{ControlRequest, ControlResponse, LeaseRegistration, ServerClient};
use crate::server::lifecycle::{HEARTBEAT_INTERVAL, ServerGeneration, connect_or_elect};

/// Pure heartbeat response classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatDisposition {
    /// The registered generation still owns the lease.
    Current,
    /// The process or lease is missing and must be recovered.
    Recover,
}

/// Classify one heartbeat exchange for recovery.
#[must_use]
pub const fn heartbeat_disposition(response: Option<&ControlResponse>) -> HeartbeatDisposition {
    match response {
        Some(ControlResponse::Accepted {
            shutdown: false, ..
        }) => HeartbeatDisposition::Current,
        Some(_) | None => HeartbeatDisposition::Recover,
    }
}

/// A status change emitted by the background heartbeat worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatEvent {
    /// A replacement generation was elected or reused and registration resumed.
    Recovered(ServerGeneration),
    /// The latest recovery attempt failed; the next heartbeat retries.
    RecoveryFailed(String),
}

/// Background owner of one registered TUI lease.
pub struct HeartbeatWorker {
    client: ServerClient,
    registration: LeaseRegistration,
    generation: Arc<Mutex<ServerGeneration>>,
    stop: Sender<()>,
    events: Receiver<HeartbeatEvent>,
    join: Option<JoinHandle<()>>,
    unregistered: bool,
}

impl HeartbeatWorker {
    /// Start renewing an already accepted registration.
    #[must_use]
    pub fn start(client: ServerClient, registration: LeaseRegistration) -> Self {
        let generation = Arc::new(Mutex::new(registration.generation));
        let worker_generation = Arc::clone(&generation);
        let worker_client = client.clone();
        let mut worker_registration = registration.clone();
        let (stop, stop_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let join = thread::spawn(move || {
            while stop_rx.recv_timeout(HEARTBEAT_INTERVAL).is_err() {
                let current = *worker_generation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let response = worker_client.request(&ControlRequest::Heartbeat {
                    generation: current,
                    lease_id: worker_registration.lease_id,
                });
                if heartbeat_disposition(response.as_ref().ok()) == HeartbeatDisposition::Current {
                    continue;
                }
                match recover(&worker_client, &mut worker_registration) {
                    Ok(recovered) => {
                        *worker_generation
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = recovered;
                        let _ = event_tx.send(HeartbeatEvent::Recovered(recovered));
                    }
                    Err(error) => {
                        let _ = event_tx.send(HeartbeatEvent::RecoveryFailed(error.to_string()));
                    }
                }
            }
        });
        Self {
            client,
            registration,
            generation,
            stop,
            events,
            join: Some(join),
            unregistered: false,
        }
    }

    /// Drain worker status without blocking the TUI event loop.
    pub fn poll(&self) -> impl Iterator<Item = HeartbeatEvent> + '_ {
        self.events.try_iter()
    }

    /// Stop heartbeats, then unregister before the caller removes its job socket.
    ///
    /// # Errors
    ///
    /// Returns the bounded unregister error after the worker has stopped.
    pub fn shutdown(&mut self) -> Result<()> {
        if self.unregistered {
            return Ok(());
        }
        let _ = self.stop.send(());
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| anyhow::anyhow!("heartbeat worker panicked"))?;
        }
        self.unregistered = true;
        let generation = *self
            .generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.client
            .unregister_generation(generation, self.registration.lease_id)
            .context("unregistering the TUI lease")?;
        Ok(())
    }
}

impl Drop for HeartbeatWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn recover(
    client: &ServerClient,
    registration: &mut LeaseRegistration,
) -> Result<ServerGeneration> {
    let record = connect_or_elect(client).context("recovering the shared brain server")?;
    registration.generation = record.generation;
    client
        .register_generation(registration)
        .context("re-registering the live TUI")?;
    Ok(record.generation)
}
