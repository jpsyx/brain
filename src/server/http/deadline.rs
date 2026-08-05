//! One absolute monotonic budget for every operation on an HTTP connection.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) trait ConnectionClock: Send + Sync {
    fn now(&self) -> Instant;
}

pub(super) struct SystemClock;

impl ConnectionClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
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
        self.deadline = self
            .clock
            .now()
            .checked_add(budget)
            .ok_or_else(|| std::io::Error::other("HTTP handler deadline overflow"))?;
        Ok(())
    }

    pub(super) fn ensure_remaining(&self, required: Duration) -> std::io::Result<()> {
        let remaining = self.ensure_open()?;
        if remaining < required {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "HTTP handler deadline cannot cover job handoff and response",
            ));
        }
        Ok(())
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
