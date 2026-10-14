//! No-shell process coordinator with one deadline for all three pipes.
use std::ffi::OsString;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use super::protocol::{AdapterError, ErrorKind, Request, Response, decode, encode};
use crate::tasks::removal::model::AdapterSnapshot;

mod capture;
mod cleanup;
use capture::{Capture, nonblocking};

/// Credentials are read only at invocation, never stored in consent snapshots.
pub(crate) fn invoke(
    adapter: &AdapterSnapshot,
    request: &Request,
) -> Result<Response, AdapterError> {
    invoke_with_environment(adapter, request, |name| std::env::var_os(name))
}

pub(super) fn invoke_with_environment(
    adapter: &AdapterSnapshot,
    request: &Request,
    mut environment: impl FnMut(&str) -> Option<OsString>,
) -> Result<Response, AdapterError> {
    let bytes = encode(request)?;
    let Some(executable) = adapter
        .argv
        .first()
        .filter(|path| std::path::Path::new(path).is_absolute())
    else {
        return Err(AdapterError::new(ErrorKind::Configuration));
    };
    if !(1..=120_000).contains(&adapter.timeout_ms) {
        return Err(AdapterError::new(ErrorKind::Configuration));
    }
    let forwarded: Vec<_> = adapter
        .environment_names
        .iter()
        .filter_map(|name| environment(name).map(|value| (name, value)))
        .collect();
    let deadline = Instant::now() + Duration::from_millis(adapter.timeout_ms);
    let reaper = cleanup::Reaper::start().map_err(|_| AdapterError::new(ErrorKind::Spawn))?;
    let mut child = Command::new(executable)
        .args(&adapter.argv[1..])
        .env_clear()
        .envs(forwarded.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|_| AdapterError::new(ErrorKind::Spawn))?;
    let result = communicate(&mut child, &bytes, deadline).and_then(|(status, capture)| {
        let stderr = redact(&capture.stderr, &forwarded);
        if !status.success() {
            return Err(AdapterError {
                kind: ErrorKind::Exit,
                stderr,
            });
        }
        let mut response = decode(request, &capture.stdout).map_err(|mut error| {
            error.stderr = stderr;
            error
        })?;
        response.message = response
            .message
            .map(|message| redact(message.as_bytes(), &forwarded));
        Ok(response)
    });
    // A protocol or exit failure also owns cleanup, even after the leader exited.
    if result.is_err()
        || result
            .as_ref()
            .is_ok_and(|response| response.status == "error")
    {
        reaper.terminate(child, deadline);
    }
    result
}

fn communicate(
    child: &mut Child,
    request: &[u8],
    deadline: Instant,
) -> Result<(std::process::ExitStatus, Capture), AdapterError> {
    let mut stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AdapterError::new(ErrorKind::Io))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AdapterError::new(ErrorKind::Io))?;
    nonblocking(
        stdin
            .as_ref()
            .ok_or_else(|| AdapterError::new(ErrorKind::Io))?
            .as_raw_fd(),
    )?;
    nonblocking(stdout.as_raw_fd())?;
    nonblocking(stderr.as_raw_fd())?;
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker = thread::Builder::new()
        .name("removal-adapter-output".into())
        .spawn(move || capture::drain(stdout, stderr, deadline, &worker_stop))
        .map_err(|_| AdapterError::new(ErrorKind::Io))?;
    let mut worker = Some(worker);
    let mut capture = None;
    let mut position = 0;
    let mut status = None;
    let result = loop {
        if Instant::now() >= deadline {
            break Err(AdapterError::new(ErrorKind::Timeout));
        }
        if worker.as_ref().is_some_and(thread::JoinHandle::is_finished) {
            let completed = worker.take().expect("finished worker exists").join();
            match completed {
                Ok(Ok(output)) => capture = Some(output),
                Ok(Err(error)) => break Err(error),
                Err(_) => break Err(AdapterError::new(ErrorKind::Io)),
            }
        }
        if let Some(input) = &mut stdin {
            match input.write(&request[position..]) {
                Ok(0) => break Err(AdapterError::new(ErrorKind::Io)),
                Ok(count) => {
                    position += count;
                    if position == request.len() {
                        stdin = None;
                    }
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break Err(AdapterError::new(ErrorKind::Io)),
            }
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => break Err(AdapterError::new(ErrorKind::Io)),
            }
        }
        if let Some(status) = status {
            if capture.is_some() && stdin.is_none() {
                break Ok((status, capture.take().expect("completed output exists")));
            }
        }
        thread::sleep(Duration::from_millis(1));
    };
    // The worker checks stop/deadline between finite nonblocking reads. Even a
    // descendant that escapes the owned group cannot make this join hang.
    stop.store(true, Ordering::Relaxed);
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    result
}

fn redact(bytes: &[u8], forwarded: &[(&String, OsString)]) -> String {
    use std::os::unix::ffi::OsStrExt;
    // Replace bytes before lossy UTF-8 conversion, including non-Unicode secrets.
    const OMITTED: &str = "[REDACTED OUTPUT OMITTED]";
    let mut redacted = Vec::new();
    let mut secrets: Vec<_> = forwarded
        .iter()
        .map(|(_, value)| value.as_os_str().as_bytes())
        .filter(|value| !value.is_empty())
        .collect();
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    let mut offset = 0;
    while offset < bytes.len() {
        let matched = secrets
            .iter()
            .find(|secret| bytes[offset..].starts_with(secret));
        let replacement = matched.map_or_else(|| &bytes[offset..=offset], |_| b"[REDACTED]");
        if replacement.len() > super::protocol::LIMIT - redacted.len() {
            return OMITTED.into();
        }
        redacted.extend_from_slice(replacement);
        offset += matched.map_or(1, |secret| secret.len());
    }
    let output = String::from_utf8_lossy(&redacted);
    if output.len() > super::protocol::LIMIT {
        return OMITTED.into();
    }
    output.into_owned()
}
