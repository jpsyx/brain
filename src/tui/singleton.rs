//! The one-interactive-brain-shell guard.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::{
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

#[must_use]
pub fn lock_path(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    workspace.paths().tui_lock()
}

#[must_use]
pub fn lock_is_reclaimable(existing_pid: Option<i32>, pid_alive: bool) -> bool {
    existing_pid.is_none() || !pid_alive
}

pub struct Guard {
    file: File,
    path: PathBuf,
}

impl Guard {
    pub fn acquire(workspace: &crate::workspace::WorkspaceContext) -> Result<Self> {
        Self::acquire_path(&workspace.paths().tui_lock())
    }

    fn acquire_path(path: &std::path::Path) -> Result<Self> {
        let path = path.to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id()).context("writing brain singleton lock")?;
                Ok(Self { file, path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let pid = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|raw| raw.trim().parse().ok());
                if lock_is_reclaimable(pid, pid.is_some_and(crate::state::system_pid_alive)) {
                    let _ = std::fs::remove_file(&path);
                    return Self::acquire_path(&path);
                }
                bail!("brain is already running (lock: {})", path.display());
            }
            Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.file.flush();
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Workspace-scoped endpoint owned for exactly one TUI lifetime.
#[derive(Debug)]
pub struct JobSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl JobSocket {
    /// Bind the selected workspace's UUID-scoped socket.
    ///
    /// # Errors
    ///
    /// Returns an error when a live owner exists or the endpoint cannot be
    /// created and secured.
    pub fn bind(workspace: &crate::workspace::WorkspaceContext) -> Result<Self> {
        Self::bind_at(workspace.paths().job_socket())
    }

    fn bind_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        if path.exists() {
            match UnixStream::connect(&path) {
                Ok(_) => anyhow::bail!("another brain TUI already owns {}", path.display()),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    ) =>
                {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("removing stale {}", path.display()))?;
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("checking {}", path.display()));
                }
            }
        }
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("securing {}", path.display()))?;
        listener
            .set_nonblocking(true)
            .context("making workspace job socket nonblocking")?;
        Ok(Self { listener, path })
    }

    /// Drain the transitional receiver commands queued on this job endpoint.
    #[must_use]
    pub fn poll(&self) -> Vec<(UnixStream, String)> {
        let mut requests = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
                    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
                    let mut command = String::new();
                    if let Err(error) = Read::by_ref(&mut stream)
                        .take(128)
                        .read_to_string(&mut command)
                    {
                        crate::logging::log(format!("workspace job read failed: {error}"));
                        continue;
                    }
                    requests.push((stream, command.trim().to_owned()));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    crate::logging::log(format!("workspace job accept failed: {error}"));
                    break;
                }
            }
        }
        requests
    }
}

impl Drop for JobSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Send one transitional receiver command to the selected workspace's TUI.
///
/// # Errors
///
/// Returns an error when no TUI is listening or the bounded exchange fails.
pub fn send_job_control(
    workspace: &crate::workspace::WorkspaceContext,
    command: &str,
) -> Result<String> {
    let mut stream = UnixStream::connect(workspace.paths().job_socket())
        .context("connecting to the running brain TUI")?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .context("setting receiver command write timeout")?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .context("setting receiver command read timeout")?;
    stream
        .write_all(command.as_bytes())
        .context("sending receiver command")?;
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("reading receiver command response")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pid_is_reclaimable() {
        assert!(lock_is_reclaimable(None, false));
    }

    #[test]
    fn live_pid_is_not_reclaimable() {
        assert!(!lock_is_reclaimable(Some(42), true));
    }

    #[test]
    fn dead_pid_is_reclaimable() {
        assert!(lock_is_reclaimable(Some(42), false));
    }

    #[test]
    fn a_live_job_socket_cannot_be_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("jobs.sock");
        let _owner = JobSocket::bind_at(path.clone()).unwrap();

        let error = JobSocket::bind_at(path).unwrap_err();

        assert!(error.to_string().contains("already owns"));
    }

    #[test]
    fn job_socket_is_owner_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("jobs.sock");
        let _socket = JobSocket::bind_at(path.clone()).unwrap();
        let mode = std::fs::metadata(path).unwrap().permissions().mode();

        assert_eq!(mode & 0o777, 0o600);
    }
}
