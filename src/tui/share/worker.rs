use crate::share::ShareResult;
use crate::workspace::WorkspaceContext;
use anyhow::{Context, Result, bail};
use std::io::{BufRead as _, BufReader, Read as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

enum Event {
    Progress(String),
    Output(std::io::Result<String>),
    StderrDone,
}

pub(crate) struct Worker {
    child: Child,
    events: Receiver<Event>,
    output: Option<std::io::Result<String>>,
    status: Option<ExitStatus>,
    stderr_done: bool,
    pub(super) path: PathBuf,
}

impl Worker {
    pub(super) fn start(
        workspace: &WorkspaceContext,
        path: &Path,
        duration: Duration,
    ) -> Result<Self> {
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--workspace",
                workspace.name().as_str(),
                "share",
                "--json",
                "--expires",
                &format!("{}s", duration.as_secs()),
                "--",
            ])
            .arg(path)
            .env("BRAIN_WORKSPACE_ID", workspace.id().to_string())
            .env(crate::workspace::STRICT_ENV, "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .context("Start sharing process")?;
        Self::from_child(child, path.to_path_buf())
    }

    fn from_child(mut child: Child, path: PathBuf) -> Result<Self> {
        let stdout = child
            .stdout
            .take()
            .context("Sharing process stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("Sharing process stderr unavailable")?;
        let (sender, events) = mpsc::channel();
        let output_sender = sender.clone();
        std::thread::spawn(move || {
            let mut output = String::new();
            let result = stdout
                .take(65_536)
                .read_to_string(&mut output)
                .map(|_| output);
            let _ = output_sender.send(Event::Output(result));
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(Event::Progress(line)).is_err() {
                    break;
                }
            }
            let _ = sender.send(Event::StderrDone);
        });
        Ok(Self {
            child,
            events,
            output: None,
            status: None,
            stderr_done: false,
            path,
        })
    }

    pub(super) fn poll(&mut self, progress: &mut String) -> Result<Option<ShareResult>> {
        for event in self.events.try_iter() {
            match event {
                Event::Progress(line) if !line.trim().is_empty() => *progress = line,
                Event::Progress(_) => {}
                Event::Output(output) => self.output = Some(output),
                Event::StderrDone => self.stderr_done = true,
            }
        }
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
        }
        let Some(status) = self.status else {
            return Ok(None);
        };
        if !self.stderr_done {
            return Ok(None);
        }
        let Some(output) = self.output.take() else {
            return Ok(None);
        };
        if !status.success() {
            bail!("Sharing failed: {progress}");
        }
        let result =
            serde_json::from_str(&output?).context("Sharing returned an invalid result")?;
        Ok(Some(result))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.status.is_none() && self.child.try_wait().ok().flatten().is_none() {
            if let Ok(pid) = i32::try_from(self.child.id()) {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn child(script: &str) -> Child {
        Command::new("/bin/sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap()
    }

    fn finish(worker: &mut Worker, progress: &mut String) -> Result<ShareResult> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = worker.poll(progress)? {
                return Ok(result);
            }
            assert!(Instant::now() < deadline, "worker did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn completed_child_delivers_json_and_separate_progress() {
        let child = child(
            r#"printf '%s\n' 'Uploading selected file' >&2
printf '%s\n' '{"path":"/example/paper.pdf","url":"https://example.test/download","expires_at":"2026-10-17T00:00:00+00:00"}'"#,
        );
        let mut worker = Worker::from_child(child, PathBuf::from("/example/paper.pdf")).unwrap();
        let mut progress = String::new();
        let result = finish(&mut worker, &mut progress).unwrap();
        assert_eq!(result.url, "https://example.test/download");
        assert_eq!(progress, "Uploading selected file");
    }

    #[test]
    fn failure_is_reported_without_treating_stdout_as_a_link() {
        let child = child("printf '%s\\n' 'network unavailable' >&2; exit 1");
        let mut worker = Worker::from_child(child, PathBuf::new()).unwrap();
        let error = finish(&mut worker, &mut String::new()).err().unwrap();
        assert!(error.to_string().contains("network unavailable"));
    }

    #[test]
    fn dropping_a_running_worker_terminates_and_reaps_its_child() {
        let child = child("exec sleep 30");
        let pid = child.id();
        let worker = Worker::from_child(child, PathBuf::new()).unwrap();
        drop(worker);
        assert!(!crate::server::lifecycle::pid_alive(pid));
    }
}
