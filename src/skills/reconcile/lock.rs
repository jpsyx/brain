//! Bounded contention only for automatic compatibility, before any install work.
use super::super::install_transaction::{InstallBusy, InstallIdentity, LockedInstall};
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(25);
const NOTICE: &str = "Waiting for another skill update to finish (up to 30 seconds).";

#[derive(Debug)]
struct LockTimeout {
    workspace_id: String,
}

impl std::fmt::Display for LockTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "timed out waiting for workspace publication ownership for {}; retry after the active updater finishes",
            self.workspace_id
        )
    }
}
impl std::error::Error for LockTimeout {}

/// Whether `error` only reports that another sync or skill update owns the
/// workspace: the bounded wait expired, or an immediate attempt found it busy.
pub(super) fn is_contention(error: &anyhow::Error) -> bool {
    error.downcast_ref::<LockTimeout>().is_some()
        || error
            .downcast_ref::<super::super::install_transaction::SyncBusy>()
            .is_some()
        || error.downcast_ref::<InstallBusy>().is_some()
}

pub(super) fn acquire(identity: &InstallIdentity) -> Result<LockedInstall> {
    let deadline = Instant::now()
        .checked_add(WAIT)
        .context("skill install wait deadline overflow")?;
    acquire_until_with_notice(identity, deadline, Instant::now, std::thread::sleep, || {
        eprintln!("{}", crate::theme::Theme::active().info(NOTICE));
    })
}

pub(super) fn acquire_sync(identity: &InstallIdentity) -> Result<crate::sync::lock::Guard> {
    let deadline = Instant::now()
        .checked_add(WAIT)
        .context("skill sync wait deadline overflow")?;
    wait_until(
        identity,
        deadline,
        Instant::now,
        std::thread::sleep,
        || {
            eprintln!(
                "{}",
                crate::theme::Theme::active()
                    .info("Waiting for another workspace sync to finish (up to 30 seconds).")
            );
        },
        || identity.acquire_sync(),
        |error| {
            error
                .downcast_ref::<super::super::install_transaction::SyncBusy>()
                .is_some()
        },
    )
}

fn acquire_until_with_notice(
    identity: &InstallIdentity,
    deadline: Instant,
    now: impl FnMut() -> Instant,
    poll: impl FnMut(Duration),
    notice: impl FnMut(),
) -> Result<LockedInstall> {
    wait_until(
        identity,
        deadline,
        now,
        poll,
        notice,
        || LockedInstall::acquire(identity),
        |error| error.downcast_ref::<InstallBusy>().is_some(),
    )
}

fn wait_until<T>(
    identity: &InstallIdentity,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut poll: impl FnMut(Duration),
    mut notice: impl FnMut(),
    mut acquire: impl FnMut() -> Result<T>,
    busy: impl Fn(&anyhow::Error) -> bool,
) -> Result<T> {
    let expired = || LockTimeout {
        workspace_id: identity.workspace_id().to_owned(),
    };
    let mut announced = false;
    loop {
        if now() >= deadline {
            return Err(expired().into());
        }
        match acquire() {
            Ok(locked) => {
                // acquire revalidates portable/root identity after obtaining ownership.
                if now() >= deadline {
                    return Err(expired().into());
                }
                return Ok(locked);
            }
            Err(error) if busy(&error) => {
                if !announced {
                    notice();
                    announced = true;
                }
                let remaining = deadline.saturating_duration_since(now());
                if remaining.is_zero() {
                    return Err(expired().into());
                }
                poll(POLL.min(remaining));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
