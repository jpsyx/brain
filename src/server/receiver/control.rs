use std::io::{Read, Write};
use std::os::unix::{
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

#[must_use]
pub fn control_path() -> PathBuf {
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".cache/brain/receiver.sock"),
        |home| PathBuf::from(home).join(".cache/brain/receiver.sock"),
    )
}

#[derive(Debug)]
pub struct ControlSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl ControlSocket {
    pub fn bind() -> Result<Self> {
        Self::bind_at(control_path())
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
            .context("making receiver control socket nonblocking")?;
        Ok(Self { listener, path })
    }

    fn drain(&self) -> Vec<(UnixStream, String)> {
        let mut requests = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let mut command = String::new();
                    if let Err(error) = Read::by_ref(&mut stream)
                        .take(128)
                        .read_to_string(&mut command)
                    {
                        crate::logging::log(format!("receiver control read failed: {error}"));
                        continue;
                    }
                    requests.push((stream, command.trim().to_owned()));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    crate::logging::log(format!("receiver control accept failed: {error}"));
                    break;
                }
            }
        }
        requests
    }

    #[must_use]
    pub fn poll(&self) -> Vec<(UnixStream, String)> {
        self.drain()
    }
}

impl Drop for ControlSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn send_control(command: &str) -> Result<String> {
    let mut stream =
        UnixStream::connect(control_path()).context("connecting to the running brain TUI")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .context("setting receiver command write timeout")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
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
    use super::ControlSocket;

    #[test]
    fn a_live_control_socket_cannot_be_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("receiver.sock");
        let _owner = ControlSocket::bind_at(path.clone()).unwrap();

        let error = ControlSocket::bind_at(path).unwrap_err();

        assert!(error.to_string().contains("already owns"));
    }

    #[test]
    fn control_socket_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("receiver.sock");
        let _socket = ControlSocket::bind_at(path.clone()).unwrap();
        let mode = std::fs::metadata(path).unwrap().permissions().mode();

        assert_eq!(mode & 0o777, 0o600);
    }
}
