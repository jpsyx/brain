use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;

use crate::server::http::deadline::HandoffDeadline;

const ACK_LIMIT: usize = 256;

#[cfg(test)]
pub(super) fn forward_serialized_until(
    path: &Path,
    frame: &[u8],
    deadline: &HandoffDeadline,
) -> std::io::Result<()> {
    forward_serialized_until_with_admission(path, frame, deadline, || Ok(()), || Ok(()))
}

pub(super) fn forward_serialized_until_with_admission(
    path: &Path,
    frame: &[u8],
    deadline: &HandoffDeadline,
    final_admission: impl FnOnce() -> std::io::Result<()>,
    commit_admission: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    deadline.ensure_open()?;
    let mut stream = crate::server::control::connect::connect_until(path, deadline.expires_at())
        .map_err(|error| std::io::Error::other(format!("{error:#}")))?;
    deadline.ensure_open()?;
    write_until_with_chunk(&mut stream, frame, deadline, usize::MAX)?;
    write_until_with_chunk(&mut stream, b"\n", deadline, usize::MAX)?;
    let prepared = read_ack_line_until(&mut stream, deadline)?;
    if prepared.trim() != "prepared" {
        return Err(std::io::Error::other(prepared));
    }
    if let Err(error) = final_admission() {
        let _ = write_until_with_chunk(&mut stream, b"cancel\n", deadline, usize::MAX);
        return Err(error);
    }
    if let Err(error) = commit_admission() {
        let _ = write_until_with_chunk(&mut stream, b"cancel\n", deadline, usize::MAX);
        return Err(error);
    }
    write_until_with_chunk(&mut stream, b"commit\n", deadline, usize::MAX)?;
    let response = read_ack_until_with_chunk(&mut stream, deadline, ACK_LIMIT + 1)?;
    if std::str::from_utf8(&response).map(str::trim) != Ok("accepted") {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&response).into_owned(),
        ));
    }
    Ok(())
}

fn read_ack_line_until(
    stream: &mut UnixStream,
    deadline: &HandoffDeadline,
) -> std::io::Result<String> {
    stream.set_nonblocking(true)?;
    let mut response = Vec::new();
    loop {
        deadline.ensure_open()?;
        let mut byte = [0_u8; 1];
        match stream.read(&mut byte) {
            Ok(0) => return Err(std::io::Error::other("job socket closed before admission")),
            Ok(_) if byte[0] == b'\n' => {
                return String::from_utf8(response).map_err(std::io::Error::other);
            }
            Ok(_) if response.len() < ACK_LIMIT => response.push(byte[0]),
            Ok(_) => {
                return Err(std::io::Error::other(
                    "job admission response exceeds frame limit",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => wait_for_io(deadline)?,
            Err(error) => return Err(error),
        }
    }
}

fn write_until_with_chunk(
    stream: &mut UnixStream,
    frame: &[u8],
    deadline: &HandoffDeadline,
    chunk_size: usize,
) -> std::io::Result<()> {
    stream.set_nonblocking(true)?;
    let mut written = 0;
    while written < frame.len() {
        deadline.ensure_open()?;
        let end = written.saturating_add(chunk_size).min(frame.len());
        match stream.write(&frame[written..end]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "job socket closed while writing",
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
    loop {
        deadline.ensure_open()?;
        match stream.flush() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_ack_until_with_chunk(
    stream: &mut UnixStream,
    deadline: &HandoffDeadline,
    chunk_size: usize,
) -> std::io::Result<Vec<u8>> {
    stream.set_nonblocking(true)?;
    let mut response = Vec::with_capacity(ACK_LIMIT);
    loop {
        deadline.ensure_open()?;
        let mut chunk = [0_u8; ACK_LIMIT + 1];
        let limit = chunk_size.min(chunk.len());
        match stream.read(&mut chunk[..limit]) {
            Ok(0) => return Ok(response),
            Ok(count) => {
                if response.len().saturating_add(count) > ACK_LIMIT {
                    return Err(std::io::Error::other(
                        "job enqueue acknowledgment exceeds frame limit",
                    ));
                }
                response.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                wait_for_io(deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn wait_for_io(deadline: &HandoffDeadline) -> std::io::Result<()> {
    let remaining = deadline.ensure_open()?;
    std::thread::park_timeout(remaining.min(std::time::Duration::from_millis(1)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Write as _;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{Arc, Barrier, Mutex};
    use std::time::{Duration, Instant};

    use super::{forward_serialized_until, read_ack_until_with_chunk, write_until_with_chunk};
    use crate::server::http::deadline::{ConnectionClock, HandoffDeadline};

    #[test]
    fn continuous_write_progress_cannot_renew_the_handoff_deadline() {
        let (mut writer, _reader) = UnixStream::pair().expect("Unix stream pair");
        let started = Instant::now();
        let deadline = started + Duration::from_secs(1);
        let clock = Arc::new(StepClock::new([started, deadline]));
        let handoff = HandoffDeadline::new(clock, deadline);

        let error = write_until_with_chunk(&mut writer, b"accepted", &handoff, 1)
            .expect_err("successful byte writes must not renew the absolute deadline");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn continuous_ack_progress_cannot_renew_the_handoff_deadline() {
        let (mut reader, mut writer) = UnixStream::pair().expect("Unix stream pair");
        writer
            .write_all(b"accepted")
            .expect("buffer acknowledgment");
        writer
            .shutdown(std::net::Shutdown::Write)
            .expect("finish acknowledgment");
        let started = Instant::now();
        let deadline = started + Duration::from_secs(1);
        let clock = Arc::new(StepClock::new([started, deadline]));
        let handoff = HandoffDeadline::new(clock, deadline);

        let error = read_ack_until_with_chunk(&mut reader, &handoff, 1)
            .expect_err("successful byte reads must not renew the absolute deadline");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn expiry_after_provider_work_never_enters_the_job_socket_boundary() {
        let temporary = tempfile::tempdir().expect("temporary socket directory");
        let path = temporary.path().join("jobs.sock");
        let listener = UnixListener::bind(&path).expect("job listener");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let clock = Arc::new(ManualClock::new());
        let handoff = HandoffDeadline::new(clock.clone(), clock.now() + Duration::from_secs(2));
        let provider_done = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_provider_done = Arc::clone(&provider_done);
        let worker_release = Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            worker_provider_done.wait();
            worker_release.wait();
            forward_serialized_until(&path, b"{}", &handoff)
        });

        provider_done.wait();
        clock.advance(Duration::from_secs(2));
        release.wait();

        let error = worker
            .join()
            .expect("handoff worker")
            .expect_err("expired provider work must not begin socket handoff");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    struct StepClock {
        instants: Mutex<VecDeque<Instant>>,
    }

    impl StepClock {
        fn new(instants: impl IntoIterator<Item = Instant>) -> Self {
            Self {
                instants: Mutex::new(instants.into_iter().collect()),
            }
        }
    }

    impl ConnectionClock for StepClock {
        fn now(&self) -> Instant {
            self.instants
                .lock()
                .expect("clock lock")
                .pop_front()
                .expect("one instant per transport boundary")
        }
    }

    struct ManualClock {
        now: Mutex<Instant>,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(Instant::now()),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.now.lock().expect("clock lock");
            *now += duration;
        }
    }

    impl ConnectionClock for ManualClock {
        fn now(&self) -> Instant {
            *self.now.lock().expect("clock lock")
        }
    }
}
