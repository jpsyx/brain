//! Nonblocking control-socket ownership and bounded request handling.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};

use super::{ControlServer, STREAM_TIMEOUT};
use crate::server::control::ControlResponse;
use crate::server::lifecycle::ServerDecision;

/// Nonblocking owner of the machine-global control socket.
#[derive(Debug)]
pub struct ControlListener {
    listener: UnixListener,
    shutdown: Arc<AtomicBool>,
}

impl ControlListener {
    /// Bind and secure the control socket for one elected process.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket cannot be bound, secured, or configured.
    pub fn bind(paths: &crate::server::lifecycle::ServerPaths) -> Result<Self> {
        let listener = UnixListener::bind(paths.control_socket())
            .with_context(|| format!("binding {}", paths.control_socket().display()))?;
        fs::set_permissions(paths.control_socket(), fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing {}", paths.control_socket().display()))?;
        listener
            .set_nonblocking(true)
            .context("making server control socket nonblocking")?;
        Ok(Self {
            listener,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Drain every ready request and return the strongest lifecycle decision.
    ///
    /// Malformed client frames receive a rejection and do not terminate the
    /// process. Accept failures are returned to the process loop.
    ///
    /// # Errors
    ///
    /// Returns an error when accepting the next ready local connection fails.
    pub fn drain(&self, server: &Arc<Mutex<ControlServer>>) -> Result<ServerDecision> {
        if self.shutdown.load(Ordering::Acquire) {
            return Ok(ServerDecision::ShutdownNow);
        }
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let server = Arc::clone(server);
                    let shutdown = Arc::clone(&self.shutdown);
                    std::thread::Builder::new()
                        .name("brain-server-control".to_owned())
                        .spawn(move || match handle_stream(&mut stream, &server) {
                            Ok(ServerDecision::ShutdownNow) => {
                                shutdown.store(true, Ordering::Release);
                            }
                            Ok(ServerDecision::KeepRunning) => {}
                            Err(error) => crate::logging::log(format!(
                                "shared-server control request failed: {error:#}"
                            )),
                        })
                        .context("starting shared-server control worker")?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(ServerDecision::KeepRunning);
                }
                Err(error) => return Err(error).context("accepting server control request"),
            }
        }
    }
}

fn handle_stream(
    stream: &mut UnixStream,
    server: &Arc<Mutex<ControlServer>>,
) -> Result<ServerDecision> {
    let deadline = Instant::now()
        .checked_add(STREAM_TIMEOUT)
        .context("server control timeout exceeds the monotonic clock range")?;
    let response = match crate::server::control::codec::read_until(stream, deadline) {
        Ok(request) => ControlServer::apply_shared_until(server, request, Instant::now(), deadline),
        Err(error) => ControlResponse::Rejected {
            message: error.to_string(),
        },
    };
    let decision = match response {
        ControlResponse::Accepted { shutdown: true, .. } => ServerDecision::ShutdownNow,
        _ => ServerDecision::KeepRunning,
    };
    crate::server::control::codec::write_until(stream, &response, deadline)?;
    Ok(decision)
}
