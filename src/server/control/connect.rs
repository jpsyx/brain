//! Safe deadline-bounded Unix-domain socket connection.

use std::os::fd::{AsFd as _, AsRawFd as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, FdFlag, OFlag};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::socket::{
    connect, getsockopt, socket, sockopt::SocketError, AddressFamily, SockFlag, SockType, UnixAddr,
};

pub(super) fn connect_until(path: &Path, deadline: Instant) -> Result<UnixStream> {
    ensure_before_deadline(deadline)?;
    let socket = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )
    .context("creating a nonblocking Unix control socket")?;
    let descriptor = socket.as_raw_fd();
    let status_flags =
        fcntl(descriptor, FcntlArg::F_GETFL).context("reading Unix socket status flags")?;
    fcntl(
        descriptor,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(status_flags) | OFlag::O_NONBLOCK),
    )
    .context("making Unix socket connect nonblocking")?;
    fcntl(descriptor, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))
        .context("making Unix socket close on exec")?;
    let address = UnixAddr::new(path)
        .with_context(|| format!("addressing Unix socket {}", path.display()))?;
    ensure_before_deadline(deadline)?;
    match connect(descriptor, &address) {
        Ok(()) => {
            ensure_before_deadline(deadline)?;
            return Ok(socket.into());
        }
        Err(Errno::EINPROGRESS | Errno::EAGAIN) => {}
        Err(error) => {
            return Err(std::io::Error::from_raw_os_error(error as i32))
                .with_context(|| format!("connecting to Unix socket {}", path.display()));
        }
    }

    let mut descriptors = [PollFd::new(socket.as_fd(), PollFlags::POLLOUT)];
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .context("Unix socket connect deadline elapsed")?;
        let timeout = PollTimeout::try_from(remaining).unwrap_or(PollTimeout::MAX);
        match poll(&mut descriptors, timeout) {
            Ok(0) | Err(Errno::EINTR) => {}
            Ok(_) => {
                ensure_before_deadline(deadline)?;
                let socket_error = getsockopt(&socket, SocketError)
                    .context("reading the Unix socket connect result")?;
                if socket_error != 0 {
                    return Err(std::io::Error::from_raw_os_error(socket_error))
                        .with_context(|| format!("connecting to Unix socket {}", path.display()));
                }
                return Ok(socket.into());
            }
            Err(error) => return Err(error).context("polling Unix socket connection"),
        }
    }
}

fn ensure_before_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        anyhow::bail!("Unix socket connect deadline elapsed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};

    use super::connect_until;

    #[test]
    fn connector_times_out_without_a_worker_when_the_listener_backlog_is_full() {
        let temporary = tempfile::tempdir().expect("temporary socket directory");
        let path = temporary.path().join("full.sock");
        let _listener = UnixListener::bind(&path).expect("Unix listener");
        let mut pending = Vec::new();

        let started = Instant::now();
        let error = loop {
            let deadline = Instant::now() + Duration::from_millis(20);
            match connect_until(&path, deadline) {
                Ok(stream) => pending.push(stream),
                Err(error) => break error,
            }
            assert!(pending.len() < 1_024, "listener backlog never saturated");
        };

        let diagnostic = format!("{error:#}");
        assert!(
            diagnostic.contains("deadline") || diagnostic.contains("Connection refused"),
            "{diagnostic}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "connector exceeded its bounded attempts"
        );
    }
}
