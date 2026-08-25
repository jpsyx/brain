use anyhow::Result;
#[cfg(test)]
use anyhow::anyhow;

use crate::agent::AgentController;
#[cfg(test)]
use crate::state::ReceiverLaunchRetryOutcome;
use crate::tui::receiver::{ReceiverSessionRegistration, ReceiverSessionStore};

pub(crate) fn cleanup_receiver_launch<Store: ReceiverSessionStore>(
    registration: Option<ReceiverSessionRegistration<'_, Store>>,
    controller: &mut AgentController,
) -> Result<()> {
    let controller_error = controller.shutdown().err();
    let session_error = registration
        .map(ReceiverSessionRegistration::cleanup)
        .and_then(Result::err);

    match (controller_error, session_error) {
        (Some(controller_error), Some(session_error)) => Err(anyhow::Error::new(controller_error)
            .context(format!(
                "receiver session cleanup also failed: {session_error:#}"
            ))),
        (Some(controller_error), None) => Err(anyhow::Error::new(controller_error)),
        (None, Some(session_error)) => Err(session_error),
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
pub(crate) fn rollback_receiver_launch<Store: ReceiverSessionStore>(
    registration: Option<ReceiverSessionRegistration<'_, Store>>,
    controller: &mut AgentController,
    retry: impl FnOnce() -> Result<Option<ReceiverLaunchRetryOutcome>>,
) -> Result<ReceiverLaunchRetryOutcome> {
    let controller_error = controller.shutdown().err();
    let session_error = registration
        .map(ReceiverSessionRegistration::cleanup)
        .and_then(Result::err);
    let retry_result = retry().and_then(|outcome| {
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
