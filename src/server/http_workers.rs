//! Fixed admission workers for the shared process's loopback HTTP listener.

use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};

use crate::server::control::ControlServer;

pub(super) const HTTP_CONNECTION_LIMIT: usize = 4;

/// Process-lifetime HTTP workers with no application request queue.
pub(super) struct HttpWorkers {
    stop: Arc<AtomicBool>,
    joins: Vec<JoinHandle<()>>,
}

impl HttpWorkers {
    /// Start the fixed worker set and release it only after every spawn works.
    ///
    /// # Errors
    ///
    /// Returns an error when an operating-system thread cannot be created.
    pub(super) fn start(
        listener: TcpListener,
        control: &Arc<Mutex<ControlServer>>,
    ) -> Result<Self> {
        Self::start_with_spawner(listener, control, &mut SystemSpawner)
    }

    fn start_with_spawner(
        listener: TcpListener,
        control: &Arc<Mutex<ControlServer>>,
        spawner: &mut impl WorkerSpawner,
    ) -> Result<Self> {
        let listener = Arc::new(listener);
        let gate = Arc::new(StartGate::default());
        let stop = Arc::new(AtomicBool::new(false));
        let mut joins = Vec::with_capacity(HTTP_CONNECTION_LIMIT);
        for index in 0..HTTP_CONNECTION_LIMIT {
            let worker = HttpWorker {
                listener: Arc::clone(&listener),
                control: Arc::clone(control),
                gate: Arc::clone(&gate),
                stop: Arc::clone(&stop),
            };
            match spawner.spawn(
                format!("brain-server-http-{}", index + 1),
                Box::new(move || worker.serve()),
            ) {
                Ok(join) => joins.push(join),
                Err(error) => {
                    gate.abort();
                    for join in joins {
                        let _ = join.join();
                    }
                    return Err(error).context("starting shared-server HTTP worker");
                }
            }
        }
        gate.start();
        Ok(Self { stop, joins })
    }

    /// Signal process-lifetime workers and leave active connections to process exit.
    pub(super) fn finish_process_exit(self) {
        self.stop.store(true, Ordering::Release);
        for join in self.joins {
            if join.is_finished() {
                let _ = join.join();
            }
        }
    }
}

struct HttpWorker {
    listener: Arc<TcpListener>,
    control: Arc<Mutex<ControlServer>>,
    gate: Arc<StartGate>,
    stop: Arc<AtomicBool>,
}

impl HttpWorker {
    fn serve(self) {
        if !self.gate.wait_until_started() {
            return;
        }
        while !self.stop.load(Ordering::Acquire) {
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    crate::logging::log(format!("shared-server HTTP accept failed: {error}"));
                    return;
                }
            };
            let mut request = match super::http::Request::read(stream) {
                Ok(request) => request,
                Err(super::http::RequestError::Io(error)) => {
                    crate::logging::log(format!("shared-server HTTP request failed: {error}"));
                    continue;
                }
                Err(super::http::RequestError::Malformed | super::http::RequestError::TooLarge) => {
                    continue;
                }
            };
            let response = super::respond(&mut request, &self.control, std::time::Instant::now());
            if let Err(error) = request.write_response(&response) {
                crate::logging::log(format!("shared-server HTTP response failed: {error}"));
            }
        }
    }
}

#[derive(Default)]
struct StartGate {
    state: Mutex<StartState>,
    changed: Condvar,
}

impl StartGate {
    fn start(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = StartState::Started;
        self.changed.notify_all();
    }

    fn abort(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = StartState::Aborted;
        self.changed.notify_all();
    }

    fn wait_until_started(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *state == StartState::Starting {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *state == StartState::Started
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum StartState {
    #[default]
    Starting,
    Started,
    Aborted,
}

trait WorkerSpawner {
    fn spawn(
        &mut self,
        name: String,
        task: Box<dyn FnOnce() + Send>,
    ) -> std::io::Result<JoinHandle<()>>;
}

struct SystemSpawner;

impl WorkerSpawner for SystemSpawner {
    fn spawn(
        &mut self,
        name: String,
        task: Box<dyn FnOnce() + Send>,
    ) -> std::io::Result<JoinHandle<()>> {
        thread::Builder::new().name(name).spawn(task)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use super::{HttpWorkers, WorkerSpawner};
    use crate::server::control::ControlServer;
    use crate::server::lifecycle::ServerGeneration;
    use crate::workspace::RegistryStore;

    #[test]
    fn partial_worker_start_failure_aborts_before_any_worker_can_read_a_body() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test HTTP listener");
        let address = listener.local_addr().expect("test listener address");
        let held_client = Arc::new(Mutex::new(None));
        let control = Arc::new(Mutex::new(ControlServer::new(
            ServerGeneration::new(),
            RegistryStore::from_path(PathBuf::from("/unused/env.json")),
            PathBuf::from("/tmp"),
        )));
        let (done_tx, done_rx) = mpsc::sync_channel(0);
        let held_by_spawner = Arc::clone(&held_client);

        std::thread::spawn(move || {
            let mut spawner = FailSecondSpawn {
                address,
                attempts: 0,
                held_client: held_by_spawner,
            };
            let result = HttpWorkers::start_with_spawner(listener, &control, &mut spawner)
                .map(|_| ())
                .map_err(|error| error.to_string());
            done_tx.send(result).expect("report startup result");
        });

        let error = done_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("partial worker startup rollback must not join a body-reading worker")
            .expect_err("the injected second spawn must fail");
        assert!(
            error.contains("starting shared-server HTTP worker"),
            "{error}"
        );
        assert!(
            held_client.lock().expect("held client lock").is_some(),
            "the failure must occur while a partial client is still connected"
        );
    }

    struct FailSecondSpawn {
        address: SocketAddr,
        attempts: usize,
        held_client: Arc<Mutex<Option<TcpStream>>>,
    }

    impl WorkerSpawner for FailSecondSpawn {
        fn spawn(
            &mut self,
            _name: String,
            task: Box<dyn FnOnce() + Send>,
        ) -> std::io::Result<JoinHandle<()>> {
            self.attempts += 1;
            if self.attempts == 2 {
                let mut stream = TcpStream::connect(self.address)?;
                stream.write_all(
                    b"POST /w/00000000-0000-0000-0000-000000000000/habits/done HTTP/1.1\r\nHost: localhost\r\nContent-Length: 32\r\n\r\npartial",
                )?;
                *self.held_client.lock().expect("held client lock") = Some(stream);
                return Err(std::io::Error::other("injected second spawn failure"));
            }
            std::thread::Builder::new().spawn(task)
        }
    }
}
