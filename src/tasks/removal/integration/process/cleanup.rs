//! Deadline-bounded caller cleanup with ownership retained for eventual reaping.
use std::io;
use std::process::Child;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub(super) trait Process: Send + 'static {
    fn signal(&mut self) -> io::Result<()>;
    fn try_reap(&mut self) -> io::Result<bool>;
}

impl Process for Child {
    fn signal(&mut self) -> io::Result<()> {
        let group = i32::try_from(self.id()).ok().map(|pid| {
            nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            )
        });
        let direct = self.kill();
        if group.is_some_and(|result| result.is_ok()) {
            Ok(())
        } else {
            direct
        }
    }

    fn try_reap(&mut self) -> io::Result<bool> {
        match self.try_wait() {
            Ok(status) => Ok(status.is_some()),
            // Another waiter has already collected the child. Never signal a
            // potentially reused PID when the OS no longer recognizes ownership.
            Err(error) if error.raw_os_error() == Some(nix::errno::Errno::ECHILD as i32) => {
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }
}

/// Started before adapter spawn, so transferring an unreaped child cannot need
/// a fallible thread allocation after the invocation deadline has expired.
pub(super) struct Reaper<P: Process = Child> {
    sender: Sender<P>,
}

impl<P: Process> Reaper<P> {
    pub(super) fn start() -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<P>();
        thread::Builder::new()
            .name("removal-adapter-reaper".into())
            .spawn(move || {
                // Success or a failed spawn drops the sender and ends this
                // worker promptly. The caller never joins eventual cleanup.
                if let Ok(mut child) = receiver.recv() {
                    while !matches!(child.try_reap(), Ok(true)) {
                        let _ = child.signal();
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            })?;
        Ok(Self { sender })
    }

    pub(super) fn terminate(self, mut child: P, deadline: Instant) {
        // Signal the group even if its leader was already collected; descendants
        // can still belong to the invocation's owned group.
        let _ = child.signal();
        loop {
            match child.try_reap() {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => break,
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            thread::sleep(remaining.min(Duration::from_millis(1)));
        }
        // This unbounded channel carries at most one child, so send does not
        // wait for the worker to run. The receiver cannot exit before receiving:
        // this sender stays live, and no process methods run until after recv.
        self.sender
            .send(child)
            .unwrap_or_else(|_| unreachable!("prestarted cleanup worker must retain its receiver"));
    }
}

#[cfg(test)]
mod tests;
