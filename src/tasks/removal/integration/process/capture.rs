//! Concurrent bounded stdout/stderr draining; no inherited pipe can block a read.
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{AdapterError, ErrorKind};
use crate::tasks::removal::integration::protocol::LIMIT;

pub(super) struct Capture {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(super) fn nonblocking(fd: std::os::fd::RawFd) -> Result<(), AdapterError> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(|_| AdapterError::new(ErrorKind::Io))?;
    fcntl(
        fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|_| AdapterError::new(ErrorKind::Io))?;
    Ok(())
}

pub(super) fn drain(
    mut stdout: impl Read,
    mut stderr: impl Read,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<Capture, AdapterError> {
    let mut capture = Capture {
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let (mut stdout_eof, mut stderr_eof) = (false, false);
    loop {
        if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err(AdapterError::new(ErrorKind::Timeout));
        }
        if !stdout_eof {
            stdout_eof = read(
                &mut stdout,
                &mut capture.stdout,
                ErrorKind::ResponseTooLarge,
            )?;
        }
        if !stderr_eof {
            stderr_eof = read(&mut stderr, &mut capture.stderr, ErrorKind::StderrTooLarge)?;
        }
        if stdout_eof && stderr_eof {
            return Ok(capture);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn read(
    reader: &mut impl Read,
    captured: &mut Vec<u8>,
    overflow: ErrorKind,
) -> Result<bool, AdapterError> {
    let mut buffer = [0; 4096];
    match reader.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(count) => {
            if count > LIMIT - captured.len() {
                return Err(AdapterError::new(overflow));
            }
            captured.extend_from_slice(&buffer[..count]);
            Ok(false)
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.kind() == std::io::ErrorKind::Interrupted =>
        {
            Ok(false)
        }
        Err(_) => Err(AdapterError::new(ErrorKind::Io)),
    }
}
