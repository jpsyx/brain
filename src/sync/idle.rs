//! Periodic pull timer for long-running shells.

use std::sync::mpsc;
use std::time::Duration;

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;

/// Stops the idle-pull thread when dropped.
pub struct IdlePullHandle {
    stop: mpsc::Sender<()>,
}

impl Drop for IdlePullHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// Start a periodic callback. Thin thread shell, with the callback injected so
/// tests do not spawn rclone.
#[must_use]
pub fn spawn_idle_puller_with<F>(interval: Duration, on_fire: F) -> IdlePullHandle
where
    F: Fn() + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while rx.recv_timeout(interval).is_err() {
            on_fire();
        }
    });
    IdlePullHandle { stop: tx }
}

/// Start the real idle-pull timer when configured. Missing or zero
/// `idle_pull_secs` leaves the shell unchanged.
#[must_use]
pub fn spawn_idle_puller(cfg: &SyncConfig) -> Option<IdlePullHandle> {
    cfg.idle_pull_interval().map(|interval| {
        spawn_idle_puller_with(interval, || {
            crate::sync::trigger::spawn_detached_sync(Direction::Pull);
        })
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn idle_puller_fires_until_dropped() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let handle = super::spawn_idle_puller_with(Duration::from_millis(5), move || {
            seen.fetch_add(1, Ordering::SeqCst);
        });

        std::thread::sleep(Duration::from_millis(25));
        drop(handle);
        let count_after_drop = calls.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));

        assert!(count_after_drop > 0, "timer should fire at least once");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            count_after_drop,
            "dropping the handle should stop future timer fires"
        );
    }
}
