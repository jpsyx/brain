use super::{Process, Reaper};
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

struct DelayedProcess {
    ready: Arc<AtomicBool>,
    dropped: mpsc::Sender<()>,
    reaped: bool,
}

impl Process for DelayedProcess {
    fn signal(&mut self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }

    fn try_reap(&mut self) -> io::Result<bool> {
        self.reaped = self.ready.load(Ordering::SeqCst);
        Ok(self.reaped)
    }
}

impl Drop for DelayedProcess {
    fn drop(&mut self) {
        // Ownership may be released only after the process becomes reapable.
        if self.reaped {
            let _ = self.dropped.send(());
        }
    }
}

#[test]
fn removal_integration_cleanup_failed_kill_does_not_wait_for_delayed_reaping() {
    let ready = Arc::new(AtomicBool::new(false));
    let release = Arc::clone(&ready);
    let (dropped, received) = mpsc::channel();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        release.store(true, Ordering::SeqCst);
    });
    let reaper = Reaper::start().unwrap();
    let started = Instant::now();
    reaper.terminate(
        DelayedProcess {
            ready,
            dropped,
            reaped: false,
        },
        started + Duration::from_millis(10),
    );
    let elapsed = started.elapsed();
    releaser.join().unwrap();
    received
        .recv_timeout(Duration::from_secs(1))
        .expect("eventual cleanup retained the process");
    assert!(
        elapsed < Duration::from_millis(100),
        "cleanup exceeded caller deadline: {elapsed:?}"
    );
}

#[test]
fn removal_integration_cleanup_expired_deadline_retains_eventual_reaping() {
    let ready = Arc::new(AtomicBool::new(false));
    let release = Arc::clone(&ready);
    let (dropped, received) = mpsc::channel();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        release.store(true, Ordering::SeqCst);
    });
    let reaper = Reaper::start().unwrap();
    let started = Instant::now();
    reaper.terminate(
        DelayedProcess {
            ready,
            dropped,
            reaped: false,
        },
        started,
    );
    let elapsed = started.elapsed();
    releaser.join().unwrap();
    received
        .recv_timeout(Duration::from_secs(1))
        .expect("eventual cleanup retained the process");
    assert!(
        elapsed < Duration::from_millis(100),
        "expired cleanup blocked: {elapsed:?}"
    );
}
