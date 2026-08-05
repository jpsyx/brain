//! Bounded newline-delimited JSON framing for the local control socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;

trait DeadlineClock {
    fn now(&mut self) -> Instant;
}

struct MonotonicClock;

impl DeadlineClock for MonotonicClock {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
}

/// Maximum encoded request or response size, including its trailing newline.
pub const MAX_FRAME_BYTES: usize = 16 * 1024;

/// Encode exactly one newline-delimited JSON frame.
///
/// # Errors
///
/// Returns an error when serialization fails or the bounded frame is too large.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(message).context("serializing server control frame")?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        bail!("server control frame exceeds {MAX_FRAME_BYTES} bytes");
    }
    Ok(bytes)
}

/// Decode exactly one bounded newline-delimited JSON frame.
///
/// # Errors
///
/// Returns an error for an empty, unterminated, oversized, trailing, or invalid frame.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.is_empty() {
        bail!("server control frame is empty");
    }
    if bytes.len() > MAX_FRAME_BYTES {
        bail!("server control frame exceeds {MAX_FRAME_BYTES} bytes");
    }
    if bytes.last() != Some(&b'\n') {
        bail!("server control frame is missing its newline terminator");
    }
    if bytes[..bytes.len() - 1].contains(&b'\n') {
        bail!("server control input contains more than one frame");
    }
    serde_json::from_slice(&bytes[..bytes.len() - 1]).context("decoding server control frame")
}

/// Read and decode one bounded frame, requiring EOF after its newline.
///
/// # Errors
///
/// Returns an error for I/O failure or any invalid frame.
pub fn read<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut frame = Vec::with_capacity(1024);
    loop {
        let mut chunk = [0_u8; 1024];
        let count = reader
            .read(&mut chunk)
            .context("reading server control frame")?;
        if count == 0 {
            break;
        }
        let remaining = MAX_FRAME_BYTES.saturating_sub(frame.len());
        if count > remaining {
            bail!("server control frame exceeds {MAX_FRAME_BYTES} bytes");
        }
        frame.extend_from_slice(&chunk[..count]);
    }
    decode(&frame)
}

/// Read and decode one frame within one absolute transport deadline.
///
/// # Errors
///
/// Returns an error when timeout configuration, I/O, framing, or decoding
/// fails, or when the absolute deadline expires between chunks.
pub fn read_until<T: DeserializeOwned>(stream: &mut UnixStream, deadline: Instant) -> Result<T> {
    read_until_with_clock(stream, deadline, &mut MonotonicClock, 1024)
}

fn read_until_with_clock<T: DeserializeOwned>(
    stream: &mut UnixStream,
    deadline: Instant,
    clock: &mut impl DeadlineClock,
    chunk_size: usize,
) -> Result<T> {
    stream
        .set_nonblocking(true)
        .context("setting nonblocking server control reads")?;
    let mut frame = Vec::with_capacity(1024);
    loop {
        ensure_before_deadline(deadline, clock.now(), "reading")?;
        let mut chunk = [0_u8; 1024];
        let chunk_limit = chunk_size.min(chunk.len());
        let count = match stream.read(&mut chunk[..chunk_limit]) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline, "reading", clock)?;
                continue;
            }
            Err(error) => return Err(error).context("reading server control frame"),
        };
        if count == 0 {
            break;
        }
        let remaining_capacity = MAX_FRAME_BYTES.saturating_sub(frame.len());
        if count > remaining_capacity {
            bail!("server control frame exceeds {MAX_FRAME_BYTES} bytes");
        }
        frame.extend_from_slice(&chunk[..count]);
    }
    decode(&frame)
}

/// Encode and write one bounded frame.
///
/// # Errors
///
/// Returns an error for serialization, size, or I/O failure.
pub fn write<T: Serialize>(writer: &mut impl Write, message: &T) -> Result<()> {
    writer
        .write_all(&encode(message)?)
        .context("writing server control frame")?;
    writer.flush().context("flushing server control frame")
}

/// Encode and write one bounded frame within one absolute transport deadline.
///
/// # Errors
///
/// Returns an error when timeout configuration, serialization, size, I/O, or
/// the absolute deadline fails.
pub fn write_until<T: Serialize>(
    stream: &mut UnixStream,
    message: &T,
    deadline: Instant,
) -> Result<()> {
    write_until_with_clock(stream, message, deadline, &mut MonotonicClock, usize::MAX)
}

fn write_until_with_clock<T: Serialize>(
    stream: &mut UnixStream,
    message: &T,
    deadline: Instant,
    clock: &mut impl DeadlineClock,
    chunk_size: usize,
) -> Result<()> {
    stream
        .set_nonblocking(true)
        .context("setting nonblocking server control writes")?;
    let frame = encode(message)?;
    let mut written = 0;
    while written < frame.len() {
        ensure_before_deadline(deadline, clock.now(), "writing")?;
        let end = written.saturating_add(chunk_size).min(frame.len());
        let count = match stream.write(&frame[written..end]) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline, "writing", clock)?;
                continue;
            }
            Err(error) => return Err(error).context("writing server control frame"),
        };
        if count == 0 {
            bail!("server control socket closed while writing");
        }
        written += count;
    }
    loop {
        ensure_before_deadline(deadline, clock.now(), "flushing")?;
        match stream.flush() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline, "flushing", clock)?;
            }
            Err(error) => return Err(error).context("flushing server control frame"),
        }
    }
}

fn wait_for_io(deadline: Instant, phase: &str, clock: &mut impl DeadlineClock) -> Result<()> {
    let remaining = deadline
        .checked_duration_since(clock.now())
        .filter(|remaining| !remaining.is_zero())
        .with_context(|| format!("server control request deadline elapsed while {phase}"))?;
    std::thread::park_timeout(remaining.min(std::time::Duration::from_millis(1)));
    Ok(())
}

fn ensure_before_deadline(deadline: Instant, now: Instant, phase: &str) -> Result<()> {
    if now >= deadline {
        anyhow::bail!("server control request deadline elapsed while {phase}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Write as _;
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use super::{DeadlineClock, read_until_with_clock, write_until_with_clock};
    use crate::server::control::ControlRequest;

    struct StepClock {
        instants: VecDeque<Instant>,
    }

    impl StepClock {
        fn new(instants: impl IntoIterator<Item = Instant>) -> Self {
            Self {
                instants: instants.into_iter().collect(),
            }
        }
    }

    impl DeadlineClock for StepClock {
        fn now(&mut self) -> Instant {
            self.instants
                .pop_front()
                .expect("test clock has one instant per transport boundary")
        }
    }

    #[test]
    fn read_checks_the_absolute_deadline_during_continuous_progress() {
        let (mut reader, mut writer) = UnixStream::pair().expect("Unix stream pair");
        let frame = super::encode(&ControlRequest::Snapshot).expect("snapshot frame");
        writer.write_all(&frame).expect("buffer snapshot frame");
        writer.shutdown(Shutdown::Write).expect("finish request");
        let started = Instant::now();
        let deadline = started + Duration::from_secs(1);
        let mut clock = StepClock::new([started, deadline]);

        let error = read_until_with_clock::<ControlRequest>(&mut reader, deadline, &mut clock, 1)
            .expect_err("successful byte reads must not extend the deadline");

        assert!(error.to_string().contains("deadline"), "{error:#}");
    }

    #[test]
    fn write_and_flush_each_check_the_same_absolute_deadline() {
        let (mut writer, _reader) = UnixStream::pair().expect("Unix stream pair");
        let started = Instant::now();
        let deadline = started + Duration::from_secs(1);
        let mut progress_clock = StepClock::new([started, deadline]);

        let progress_error = write_until_with_clock(
            &mut writer,
            &ControlRequest::Snapshot,
            deadline,
            &mut progress_clock,
            1,
        )
        .expect_err("successful byte writes must not extend the deadline");

        assert!(
            progress_error.to_string().contains("deadline"),
            "{progress_error:#}"
        );

        let (mut writer, _reader) = UnixStream::pair().expect("Unix stream pair");
        let mut flush_clock = StepClock::new([started, deadline]);
        let flush_error = write_until_with_clock(
            &mut writer,
            &ControlRequest::Snapshot,
            deadline,
            &mut flush_clock,
            usize::MAX,
        )
        .expect_err("flush must not start after the deadline");

        assert!(
            flush_error.to_string().contains("deadline"),
            "{flush_error:#}"
        );
    }
}
