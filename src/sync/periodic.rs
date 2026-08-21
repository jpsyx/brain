//! Five-minute downstream reconciliation for long-running shells.

use std::sync::mpsc;
use std::time::Duration;

pub const PULL_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub struct PeriodicPullHandle {
    stop: mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for PeriodicPullHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[must_use]
pub fn spawn_periodic_puller_with<F>(interval: Duration, on_fire: F) -> PeriodicPullHandle
where
    F: Fn() + Send + 'static,
{
    let (stop, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        while receiver.recv_timeout(interval).is_err() {
            on_fire();
        }
    });
    PeriodicPullHandle {
        stop,
        worker: Some(worker),
    }
}

#[must_use]
pub fn spawn_periodic_puller(
    workspace: std::sync::Arc<crate::workspace::WorkspaceContext>,
) -> PeriodicPullHandle {
    spawn_periodic_puller_with(PULL_INTERVAL, move || {
        let _ = crate::sync::trigger::spawn_detached_sync(
            &workspace,
            crate::sync::args::Direction::Pull,
        );
    })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn configured_shell_pulls_every_five_minutes_until_its_handle_is_dropped() {
        assert_eq!(super::PULL_INTERVAL, Duration::from_secs(5 * 60));
        let (fired_tx, fired_rx) = mpsc::channel();
        let handle = super::spawn_periodic_puller_with(Duration::from_millis(1), move || {
            fired_tx.send(()).expect("observe periodic pull");
        });

        fired_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("periodic pull never fired");
        drop(handle);

        assert!(
            fired_rx.recv_timeout(Duration::from_millis(20)).is_err(),
            "dropping the shell-owned handle must stop future pulls"
        );
    }
}
