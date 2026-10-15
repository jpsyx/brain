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
            "timed out waiting for managed skill install ownership for {}; retry after the active installer finishes",
            self.workspace_id
        )
    }
}
impl std::error::Error for LockTimeout {}

pub(super) fn acquire(identity: &InstallIdentity) -> Result<LockedInstall> {
    let deadline = Instant::now()
        .checked_add(WAIT)
        .context("skill install wait deadline overflow")?;
    acquire_until_with_notice(identity, deadline, Instant::now, std::thread::sleep, || {
        eprintln!("{}", crate::theme::Theme::active().info(NOTICE));
    })
}

fn acquire_until_with_notice(
    identity: &InstallIdentity,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut poll: impl FnMut(Duration),
    mut notice: impl FnMut(),
) -> Result<LockedInstall> {
    let expired = || LockTimeout {
        workspace_id: identity.workspace_id().to_owned(),
    };
    let mut announced = false;
    loop {
        if now() >= deadline {
            return Err(expired().into());
        }
        match LockedInstall::acquire(identity) {
            Ok(locked) => {
                // acquire revalidates portable/root identity after obtaining ownership.
                if now() >= deadline {
                    return Err(expired().into());
                }
                return Ok(locked);
            }
            Err(error) if error.downcast_ref::<InstallBusy>().is_some() => {
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
