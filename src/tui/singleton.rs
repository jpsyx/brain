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

    /// Accept bounded job frames into this TUI's in-memory queue.
    ///
    /// An acknowledgment is written only after a matching job has been
    /// appended. A failed final acknowledgment removes that staged append.
    /// Full, malformed, and cross-workspace frames are discarded.
    pub fn poll_jobs(
        &self,
        workspace_id: crate::workspace::WorkspaceId,
        queue: &mut Vec<crate::server::receiver::InboundJob>,
    ) {
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    if let Err(error) = stream.set_nonblocking(false) {
                        crate::logging::log(format!(
                            "workspace job stream configuration failed: {error}"
                        ));
                        continue;
                    }
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
                    process_job_stream(&mut stream, workspace_id, queue);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    crate::logging::log(format!("workspace job accept failed: {error}"));
                    break;
                }
            }
        }
    }
}

fn process_job_stream(
    stream: &mut (impl Read + Write),
    workspace_id: crate::workspace::WorkspaceId,
    queue: &mut Vec<crate::server::receiver::InboundJob>,
) {
    let response = read_job(stream).and_then(|job| {
        if job.workspace_id != workspace_id {
            anyhow::bail!("inbound job targets another workspace");
        }
        if queue.len() >= crate::server::receiver::INBOUND_QUEUE_CAPACITY {
            anyhow::bail!("inbound queue is full");
        }
        stream
            .write_all(b"prepared\n")
            .context("acknowledging staged job")?;
        let command = read_admission_command(stream)?;
        anyhow::ensure!(command == "commit", "job admission was cancelled");
        queue.push(job);
        Ok(())
    });
    let accepted = response.is_ok();
    let message = if accepted {
        "accepted\n".to_owned()
    } else {
        let error = response.expect_err("checked error");
        format!("rejected: {error:#}\n").replace(['\r', '\n'], " ")
    };
    if let Err(error) = stream.write_all(message.as_bytes()) {
        if accepted {
            queue.pop();
        }
        crate::logging::log(format!("workspace job acknowledgment failed: {error}"));
    }
}

fn read_job(stream: &mut impl Read) -> Result<crate::server::receiver::InboundJob> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while bytes.len() <= crate::server::receiver::dispatch::JOB_FRAME_LIMIT {
        stream
            .read_exact(&mut byte)
            .context("reading inbound job")?;
        if byte[0] == b'\n' {
            break;
        }
        bytes.push(byte[0]);
    }
    anyhow::ensure!(
        bytes.len() <= crate::server::receiver::dispatch::JOB_FRAME_LIMIT,
        "inbound job exceeds the socket frame limit"
    );
    serde_json::from_slice(&bytes).context("decoding inbound job")
}

fn read_admission_command(stream: &mut impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while bytes.len() <= 16 {
        stream
            .read_exact(&mut byte)
            .context("reading job admission decision")?;
        if byte[0] == b'\n' {
            return String::from_utf8(bytes).context("decoding job admission decision");
        }
        bytes.push(byte[0]);
    }
    anyhow::bail!("job admission decision is too large")
}

impl Drop for JobSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct FailFinalAcknowledgment {
        input: Cursor<Vec<u8>>,
        writes: Vec<u8>,
        write_count: usize,
    }

    impl Read for FailFinalAcknowledgment {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buffer)
        }
    }

    impl Write for FailFinalAcknowledgment {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.write_count += 1;
            if self.write_count == 2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "injected final acknowledgment failure",
                ));
            }
            self.writes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

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

    #[test]
    fn failed_final_acknowledgment_removes_the_staged_job_deterministically() {
        let workspace_id =
            crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
        let users = crate::users::Users {
            schema_version: crate::users::USERS_SCHEMA_VERSION,
            users: vec![crate::users::User {
                id: crate::users::UserId::parse("member").unwrap(),
                name: "Member".to_owned(),
                phones: vec![crate::users::PhoneIdentity {
                    value: "+12125550100".to_owned(),
                    inbound_allowed: true,
                }],
                emails: Vec::new(),
                response_email: None,
            }],
        };
        let actor = crate::actor::resolve_actor(
            &crate::users::UserId::parse("member").unwrap(),
            crate::actor::RequestIdentity::Sms {
                from: "+12125550100",
            },
            &users,
        )
        .unwrap();
        let job = crate::server::receiver::InboundJob {
            job_id: uuid::Uuid::new_v4(),
            workspace_id,
            actor,
            channel: crate::server::receiver::Channel::Sms,
            authenticated_sender: "+12125550100".to_owned(),
            prompt: "must roll back".to_owned(),
            attachments: Vec::new(),
            received_at_unix_ms: 1,
            provider_id: None,
            thread_participants: vec!["+12125550100".to_owned()],
            response_email: None,
            allowed_response_recipients: Vec::new(),
            email_reply: None,
        };
        let mut input = serde_json::to_vec(&job).unwrap();
        input.extend_from_slice(b"\ncommit\n");
        let mut stream = FailFinalAcknowledgment {
            input: Cursor::new(input),
            writes: Vec::new(),
            write_count: 0,
        };
        let mut queue = Vec::new();

        process_job_stream(&mut stream, workspace_id, &mut queue);

        assert!(queue.is_empty());
        assert_eq!(stream.writes, b"prepared\n");
    }
}
