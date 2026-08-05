//! Fixed workers that keep HTTP body handling off the lifecycle loop.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use tiny_http::Server;

use crate::server::control::ControlServer;

const HTTP_WORKERS: usize = 4;

/// Process-lifetime HTTP workers over one shared listener and control state.
pub(super) struct HttpWorkers {
    server: Arc<Server>,
    joins: Vec<JoinHandle<()>>,
}

impl HttpWorkers {
    /// Start the fixed worker set.
    ///
    /// # Errors
    ///
    /// Returns an error when an operating-system thread cannot be created.
    pub(super) fn start(server: &Arc<Server>, control: &Arc<Mutex<ControlServer>>) -> Result<Self> {
        let mut joins: Vec<JoinHandle<()>> = Vec::with_capacity(HTTP_WORKERS);
        for index in 0..HTTP_WORKERS {
            let worker_server = Arc::clone(server);
            let worker_control = Arc::clone(control);
            let join = match thread::Builder::new()
                .name(format!("brain-server-http-{}", index + 1))
                .spawn(move || {
                    HttpWorker {
                        server: worker_server,
                        control: worker_control,
                    }
                    .serve();
                }) {
                Ok(join) => join,
                Err(error) => {
                    for _ in 0..joins.len() {
                        server.unblock();
                    }
                    for join in joins {
                        let _ = join.join();
                    }
                    return Err(error).context("starting shared-server HTTP worker");
                }
            };
            joins.push(join);
        }
        Ok(Self {
            server: Arc::clone(server),
            joins,
        })
    }

    /// Release idle workers before the process exits.
    ///
    /// A worker draining an incomplete body is intentionally not joined: the
    /// elected process is already ending, so its fixed process-lifetime worker
    /// set ends with it instead of delaying final-lease shutdown.
    pub(super) fn finish_process_exit(self) {
        for _ in 0..self.joins.len() {
            self.server.unblock();
        }
        for join in self.joins {
            if join.is_finished() {
                let _ = join.join();
            }
        }
    }
}

struct HttpWorker {
    server: Arc<Server>,
    control: Arc<Mutex<ControlServer>>,
}

impl HttpWorker {
    fn serve(self) {
        loop {
            let Ok(mut request) = self.server.recv() else {
                return;
            };
            let response = super::respond(&mut request, &self.control, std::time::Instant::now());
            if let Err(error) = request.respond(response) {
                crate::logging::log(format!("shared-server HTTP response failed: {error}"));
            }
        }
    }
}
