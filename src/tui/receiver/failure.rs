use anyhow::{Result, anyhow};

use crate::agent::AgentController;
use crate::state::{ReceiverLaunchFailure, ReceiverLaunchRetryOutcome, ReceiverRunClaim};
use crate::tui::state::AppServices;

pub(crate) fn rollback_receiver_launch(
    services: &AppServices,
    claimed: &ReceiverRunClaim,
    remote_instance: &str,
    controller: &mut AgentController,
    failure: ReceiverLaunchFailure,
    observed_at_unix_ms: u64,
    retry_at_unix_ms: u64,
) -> Result<ReceiverLaunchRetryOutcome> {
    let controller_stopped = controller.shutdown().is_ok();
    let session_released = services.release_session_lock(remote_instance).is_ok();
    let outcome = services
        .record_receiver_launch_retry(
            claimed.job().id(),
            claimed.claim().owner(),
            observed_at_unix_ms,
            retry_at_unix_ms,
            failure,
        )?
        .ok_or_else(|| anyhow!("receiver launch claim ownership was lost"))?;

    if !controller_stopped {
        return Err(anyhow!("receiver launch controller cleanup failed"));
    }
    if !session_released {
        return Err(anyhow!("receiver launch session cleanup failed"));
    }
    Ok(outcome)
}
