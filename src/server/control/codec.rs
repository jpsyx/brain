//! Bounded newline-delimited JSON framing for the local control socket.

use std::io::{Read, Write};

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

/// Read and decode one bounded frame without waiting for EOF after its newline.
///
/// # Errors
///
/// Returns an error for I/O failure or any invalid frame.
pub fn read<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut frame = Vec::with_capacity(1024);
    loop {
        let mut byte = [0_u8; 1];
        let count = reader
            .read(&mut byte)
            .context("reading server control frame")?;
        if count == 0 {
            break;
        }
        frame.push(byte[0]);
        if frame.len() > MAX_FRAME_BYTES {
            bail!("server control frame exceeds {MAX_FRAME_BYTES} bytes");
        }
        if byte[0] == b'\n' {
            break;
        }
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
