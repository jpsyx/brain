//! Linearizable receiver admission across authority revocation and TUI enqueue.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

const PENDING: u8 = 0;
const AUTHORIZED: u8 = 1;
const CANCELLED: u8 = 2;
const COMMITTED: u8 = 3;
const COMPLETED: u8 = 4;

#[derive(Debug)]
pub(crate) struct ReceiverAdmission {
    workspace_id: crate::workspace::WorkspaceId,
    lease_id: crate::server::lifecycle::LeaseId,
    state: AtomicU8,
    completion: (Mutex<bool>, Condvar),
}

impl ReceiverAdmission {
    pub(crate) const fn new(
        workspace_id: crate::workspace::WorkspaceId,
        lease_id: crate::server::lifecycle::LeaseId,
    ) -> Self {
        Self {
            workspace_id,
            lease_id,
            state: AtomicU8::new(PENDING),
            completion: (Mutex::new(false), Condvar::new()),
        }
    }

    pub(crate) const fn workspace_id(&self) -> crate::workspace::WorkspaceId {
        self.workspace_id
    }

    pub(crate) const fn lease_id(&self) -> crate::server::lifecycle::LeaseId {
        self.lease_id
    }

    pub(crate) fn authorize(&self) -> std::io::Result<()> {
        self.state
            .compare_exchange(PENDING, AUTHORIZED, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| std::io::Error::other("receiver admission was revoked"))
    }

    pub(crate) fn commit(&self) -> std::io::Result<()> {
        self.state
            .compare_exchange(AUTHORIZED, COMMITTED, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| std::io::Error::other("receiver admission was revoked"))
    }

    #[cfg(test)]
    #[expect(dead_code)]
    pub(crate) fn is_committed(&self) -> bool {
        self.state.load(Ordering::Acquire) == COMMITTED
    }

    pub(crate) fn complete(&self) {
        self.state.store(COMPLETED, Ordering::Release);
        let (lock, ready) = &self.completion;
        *lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        ready.notify_all();
    }

    pub(crate) fn revoke_or_wait_until(
        &self,
        deadline: Instant,
        clock: &impl Fn() -> Instant,
    ) -> bool {
        loop {
            match self.state.load(Ordering::Acquire) {
                PENDING | AUTHORIZED => {
                    let observed = self.state.load(Ordering::Acquire);
                    if matches!(observed, PENDING | AUTHORIZED)
                        && self
                            .state
                            .compare_exchange(
                                observed,
                                CANCELLED,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                    {
                        return true;
                    }
                }
                COMMITTED => {
                    let (lock, ready) = &self.completion;
                    let mut completed = lock
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    while !*completed {
                        let now = clock();
                        if now >= deadline {
                            return false;
                        }
                        let (next, timeout) = ready
                            .wait_timeout(completed, deadline.duration_since(now))
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        completed = next;
                        if timeout.timed_out() && !*completed {
                            return false;
                        }
                    }
                    drop(completed);
                    return true;
                }
                _ => return true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReceiverAdmission;

    fn admission() -> ReceiverAdmission {
        ReceiverAdmission::new(
            crate::workspace::WorkspaceId::new(),
            crate::server::lifecycle::LeaseId::new(),
        )
    }

    #[test]
    fn revocation_after_final_revalidation_cancels_before_socket_commit() {
        let admission = admission();
        admission.authorize().expect("final revalidation");

        assert!(admission.revoke_or_wait_until(
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            &std::time::Instant::now,
        ));

        assert!(admission.commit().is_err());
    }

    #[test]
    fn revocation_waits_for_an_already_linearized_socket_commit() {
        let admission = std::sync::Arc::new(admission());
        admission.authorize().unwrap();
        admission.commit().unwrap();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let worker_admission = std::sync::Arc::clone(&admission);
        let worker_finished = std::sync::Arc::clone(&finished);
        let worker = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            assert!(worker_admission.revoke_or_wait_until(
                std::time::Instant::now() + std::time::Duration::from_secs(1),
                &std::time::Instant::now,
            ));
            worker_finished.store(true, std::sync::atomic::Ordering::Release);
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("revocation entered");
        assert!(!finished.load(std::sync::atomic::Ordering::Acquire));

        admission.complete();
        worker.join().unwrap();
        assert!(finished.load(std::sync::atomic::Ordering::Acquire));
    }
}
