//! Bounded newline-delimited JSON framing for the local control socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;

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
    stream
        .set_nonblocking(true)
        .context("setting nonblocking server control reads")?;
    let mut frame = Vec::with_capacity(1024);
    loop {
        let mut chunk = [0_u8; 1024];
        let count = match stream.read(&mut chunk) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline, "reading")?;
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
    stream
        .set_nonblocking(true)
        .context("setting nonblocking server control writes")?;
    let frame = encode(message)?;
    let mut written = 0;
    while written < frame.len() {
        let count = match stream.write(&frame[written..]) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline, "writing")?;
                continue;
            }
            Err(error) => return Err(error).context("writing server control frame"),
        };
        if count == 0 {
            bail!("server control socket closed while writing");
        }
        written += count;
    }
    stream.flush().context("flushing server control frame")
}

fn wait_for_io(deadline: Instant, phase: &str) -> Result<()> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .with_context(|| format!("server control request deadline elapsed while {phase}"))?;
    std::thread::park_timeout(remaining.min(std::time::Duration::from_millis(1)));
    Ok(())
}
