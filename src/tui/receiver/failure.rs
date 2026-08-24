use anyhow::{Result, anyhow};

use crate::agent::AgentController;
use crate::state::{ReceiverLaunchFailure, ReceiverLaunchRetryOutcome, ReceiverRunClaim};
use crate::tui::receiver::{ReceiverSessionRegistration, ReceiverSessionStore};
use crate::tui::state::AppServices;

pub(crate) fn rollback_receiver_launch<Store: ReceiverSessionStore>(
    services: &AppServices,
    claimed: &ReceiverRunClaim,
    registration: Option<ReceiverSessionRegistration<'_, Store>>,
    controller: &mut AgentController,
    failure: ReceiverLaunchFailure,
    observed_at_unix_ms: u64,
    retry_at_unix_ms: u64,
) -> Result<ReceiverLaunchRetryOutcome> {
    let controller_error = controller.shutdown().err();
    let session_error = registration
        .map(ReceiverSessionRegistration::cleanup)
        .and_then(Result::err);
    let retry_result = services
        .record_receiver_launch_retry(
            claimed.job().id(),
            claimed.claim().owner(),
            observed_at_unix_ms,
            retry_at_unix_ms,
            failure,
        )
        .and_then(|outcome| {
            outcome.ok_or_else(|| anyhow!("receiver launch claim ownership was lost"))
        });

    if let Some(controller_error) = controller_error {
        let mut error = anyhow::Error::new(controller_error);
        if let Some(session_error) = session_error.as_ref() {
            error = error.context(format!(
                "receiver session cleanup also failed: {session_error:#}"
            ));
        }
        if let Err(retry_error) = &retry_result {
            error = error.context(format!(
                "receiver retry recording also failed: {retry_error:#}"
            ));
        }
        return Err(error);
    }
    if let Some(session_error) = session_error {
        return match retry_result {
            Ok(_) => Err(session_error),
            Err(retry_error) => Err(session_error.context(format!(
                "receiver retry recording also failed: {retry_error:#}"
            ))),
        };
    }
    retry_result
}
