//! One absolute monotonic budget for every operation on an HTTP connection.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(in crate::server) trait ConnectionClock: Send + Sync {
    fn now(&self) -> Instant;
}

pub(super) struct SystemClock;

impl ConnectionClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone)]
pub(in crate::server) struct HandoffDeadline {
    clock: Arc<dyn ConnectionClock>,
    expires_at: Instant,
}

impl HandoffDeadline {
    pub(in crate::server) fn new(clock: Arc<dyn ConnectionClock>, expires_at: Instant) -> Self {
        Self { clock, expires_at }
    }

    pub(in crate::server) fn ensure_open(&self) -> std::io::Result<Duration> {
        self.expires_at
            .checked_duration_since(self.clock.now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "receiver job handoff deadline elapsed",
                )
            })
    }
}

pub(super) struct DeadlineStream {
    stream: TcpStream,
    clock: Arc<dyn ConnectionClock>,
    deadline: Instant,
}

impl DeadlineStream {
    pub(super) fn new(
        stream: TcpStream,
        clock: Arc<dyn ConnectionClock>,
        budget: Duration,
    ) -> std::io::Result<Self> {
        let deadline = clock
            .now()
            .checked_add(budget)
            .ok_or_else(|| std::io::Error::other("HTTP connection deadline overflow"))?;
        Ok(Self {
            stream,
            clock,
            deadline,
        })
    }

    pub(super) fn ensure_open(&self) -> std::io::Result<Duration> {
        self.deadline
            .checked_duration_since(self.clock.now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "HTTP connection deadline elapsed",
                )
            })
    }

    pub(super) fn restart_budget(&mut self, budget: Duration) -> std::io::Result<()> {
        let now = self.clock.now();
        if now >= self.deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "HTTP connection deadline elapsed",
            ));
        }
        self.deadline = now
            .checked_add(budget)
            .ok_or_else(|| std::io::Error::other("HTTP handler deadline overflow"))?;
        Ok(())
    }

    pub(super) fn handoff_deadline(
        &self,
        handoff_budget: Duration,
        response_reserve: Duration,
    ) -> std::io::Result<HandoffDeadline> {
        let now = self.clock.now();
        let response_cutoff = self.deadline.checked_sub(response_reserve).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "HTTP handler deadline cannot reserve response time",
            )
        })?;
        let short_cutoff = now
            .checked_add(handoff_budget)
            .ok_or_else(|| std::io::Error::other("receiver handoff deadline overflow"))?;
        let expires_at = response_cutoff.min(short_cutoff);
        if now >= expires_at {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "HTTP handler deadline cannot cover job handoff and response",
            ));
        }
        Ok(HandoffDeadline::new(Arc::clone(&self.clock), expires_at))
    }

    fn prepare_read(&self) -> std::io::Result<()> {
        self.stream.set_read_timeout(Some(self.ensure_open()?))
    }

    fn prepare_write(&self) -> std::io::Result<()> {
        self.stream.set_write_timeout(Some(self.ensure_open()?))
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.prepare_read()?;
        let read = self.stream.read(buffer)?;
        self.ensure_open()?;
        Ok(read)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.prepare_write()?;
        let written = self.stream.write(buffer)?;
        self.ensure_open()?;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.prepare_write()?;
        self.stream.flush()?;
        self.ensure_open().map(|_| ())
    }
}
