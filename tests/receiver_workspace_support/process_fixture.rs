const PROCESS_FIXTURE_LIMIT: usize = 1;

static PROCESS_FIXTURE_PERMITS: ProcessFixturePermits =
    ProcessFixturePermits::new(PROCESS_FIXTURE_LIMIT);

struct ProcessFixturePermits {
    limit: usize,
    active: std::sync::Mutex<usize>,
    available: std::sync::Condvar,
}

impl ProcessFixturePermits {
    const fn new(limit: usize) -> Self {
        Self {
            limit,
            active: std::sync::Mutex::new(0),
            available: std::sync::Condvar::new(),
        }
    }

    fn acquire(&self) -> ProcessFixturePermit<'_> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *active == self.limit {
            active = self
                .available
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *active += 1;
        drop(active);
        ProcessFixturePermit { permits: self }
    }

    #[cfg(test)]
    fn try_acquire(&self) -> Option<ProcessFixturePermit<'_>> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *active == self.limit {
            drop(active);
            None
        } else {
            *active += 1;
            drop(active);
            Some(ProcessFixturePermit { permits: self })
        }
    }
}

pub(super) struct ProcessFixturePermit<'a> {
    permits: &'a ProcessFixturePermits,
}

impl Drop for ProcessFixturePermit<'_> {
    fn drop(&mut self) {
        let mut active = self
            .permits
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.checked_sub(1).expect("fixture permit is active");
        drop(active);
        self.permits.available.notify_one();
    }
}

pub(super) struct ProcessFixtureProcess {
    child: std::process::Child,
    _permit: ProcessFixturePermit<'static>,
}

impl ProcessFixtureProcess {
    pub(super) fn spawn(
        home: &tempfile::TempDir,
        generation: brain::server::lifecycle::ServerGeneration,
    ) -> Self {
        let permit = PROCESS_FIXTURE_PERMITS.acquire();
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_brain"))
            .args([
                "server",
                "run",
                "--generation",
                &generation.to_string(),
                "--port",
                "0",
            ])
            .env("HOME", home.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn shared receiver test server");
        Self::new(child, permit)
    }

    fn new(child: std::process::Child, permit: ProcessFixturePermit<'static>) -> Self {
        Self {
            child,
            _permit: permit,
        }
    }

    pub(super) fn terminate(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    pub(super) fn kill_and_wait(&mut self) -> std::io::Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
            self.child.wait()?;
        }
        Ok(())
    }

    pub(super) fn has_exited(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}

impl Drop for ProcessFixtureProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{
        PROCESS_FIXTURE_LIMIT, PROCESS_FIXTURE_PERMITS, ProcessFixturePermits,
        ProcessFixtureProcess,
    };

    #[test]
    fn process_fixture_permit_serializes_external_server_lifetimes() {
        let permits = ProcessFixturePermits::new(PROCESS_FIXTURE_LIMIT);
        let first = permits.acquire();

        assert!(permits.try_acquire().is_none());
        drop(first);
        assert!(permits.try_acquire().is_some());
    }

    #[test]
    fn process_fixture_reaps_child_when_startup_unwinds() {
        let permit = PROCESS_FIXTURE_PERMITS.acquire();
        let temporary = tempfile::tempdir().expect("temporary child marker");
        let marker = temporary.path().join("ready");
        let child = Command::new(std::env::current_exe().expect("integration test executable"))
            .args([
                "--exact",
                "receiver_workspace_support::process_fixture::tests::fixture_child_waits_for_parent",
            ])
            .env("BRAIN_PROCESS_FIXTURE_CHILD_READY", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn guarded child");
        let pid = i32::try_from(child.id()).expect("child PID fits i32");
        let process = ProcessFixtureProcess::new(child, permit);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "guarded child did not start");
            std::thread::yield_now();
        }

        drop(process);

        assert!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err(),
            "process fixture left its child running"
        );
    }

    #[test]
    fn fixture_child_waits_for_parent() {
        let Ok(marker) = std::env::var("BRAIN_PROCESS_FIXTURE_CHILD_READY") else {
            return;
        };
        std::fs::write(marker, b"ready").expect("signal child readiness");
        loop {
            std::thread::park();
        }
    }
}
