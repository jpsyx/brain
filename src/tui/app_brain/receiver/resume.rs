//! Resume validation and registration under one freshly observed durable owner.

use crate::agent::{AgentController, AgentSession, SessionScope};
use crate::tui::receiver::{ClaimedReceiverRun, ReceiverSessionRegistration};
use crate::tui::state::AppServices;

#[derive(Clone, Copy)]
pub(super) enum ReceiverResumeBoundary {
    Validation,
    Registration,
}

pub(super) enum ReceiverResumeDecision<'services> {
    Fresh,
    Registered {
        session: AgentSession,
        registration: ReceiverSessionRegistration<'services, AppServices>,
    },
    Lost,
    Deferred,
    RegistrationFailed,
}

pub(super) fn decide<'services>(
    controller: &AgentController,
    services: &'services AppServices,
    claimed: &ClaimedReceiverRun,
    pid: i32,
    scope: &SessionScope,
    mut observe_boundary: impl FnMut(ReceiverResumeBoundary),
) -> ReceiverResumeDecision<'services> {
    let Some(session) = claimed
        .claim
        .conversation()
        .binding()
        .filter(|binding| binding.frontend() == controller.kind())
        .and_then(|binding| AgentSession::new(binding.native_session_id()).ok())
    else {
        return ReceiverResumeDecision::Fresh;
    };

    let validation = controller.resume_candidate_exists(&session);
    observe_boundary(ReceiverResumeBoundary::Validation);
    if let Err(block) = authorize_after_boundary(services, claimed, "post-resume-validation") {
        return blocked(block);
    }
    match validation {
        Ok(true) => {}
        Ok(false) => return ReceiverResumeDecision::Fresh,
        Err(error) => {
            crate::logging::log(format!(
                "receiver native resume validation failed; using portable recovery: {error}"
            ));
            return ReceiverResumeDecision::Fresh;
        }
    }

    let registration = ReceiverSessionRegistration::claim_resume(
        services,
        claimed.claim.job().conversation_id(),
        &claimed.remote,
        &session,
        pid,
        scope,
    );
    observe_boundary(ReceiverResumeBoundary::Registration);
    if let Err(block) = authorize_after_boundary(services, claimed, "post-resume-registration") {
        if let Ok(Some(registration)) = registration {
            let _ = registration.cleanup();
        }
        return blocked(block);
    }

    match registration {
        Ok(Some(registration)) => ReceiverResumeDecision::Registered {
            session,
            registration,
        },
        Ok(None) => ReceiverResumeDecision::Fresh,
        Err(error) => {
            crate::logging::log(format!("receiver resume registration failed: {error:#}"));
            ReceiverResumeDecision::RegistrationFailed
        }
    }
}

fn authorize_after_boundary(
    services: &AppServices,
    claimed: &ClaimedReceiverRun,
    phase: &str,
) -> Result<(), super::ownership::ReceiverOwnerBlock> {
    match super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(super::ownership::ReceiverOwnerBlock::Lost),
        Err(error) => {
            crate::logging::log(format!("receiver {phase} claim check failed: {error:#}"));
            Err(super::ownership::ReceiverOwnerBlock::Deferred)
        }
    }
}

const fn blocked<'services>(
    block: super::ownership::ReceiverOwnerBlock,
) -> ReceiverResumeDecision<'services> {
    match block {
        super::ownership::ReceiverOwnerBlock::Lost => ReceiverResumeDecision::Lost,
        super::ownership::ReceiverOwnerBlock::Deferred => ReceiverResumeDecision::Deferred,
    }
}
