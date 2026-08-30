//! Same-session launch of one durably claimed receiver recovery.

use std::sync::Arc;

use crate::agent::{AgentController, AgentSession, LaunchRequest, SessionScope};
use crate::state::{ReceiverRecoveryFailure, ReceiverRunClaim};
use crate::tui::App;
use crate::tui::receiver::{
    ClaimedReceiverRun, DurableReceiverRun, PreSpawnRecoveryOutcome, ReceiverSessionRegistration,
};

mod claim;
mod effects;
pub(super) mod pre_spawn_cleanup;

enum RecoveryOwnerDecision {
    Current(super::ownership::ReceiverOwnerObservation),
    Lost,
    StoreUnavailable,
}

impl App {
    pub(super) fn launch_claimed_receiver_recovery(&mut self, claimed: ClaimedReceiverRun) {
        let Some(binding) = claimed.claim.conversation().binding() else {
            self.fail_receiver_recovery_resume(&claimed.claim);
            return;
        };
        let kind = binding.frontend();
        let Ok(session) = AgentSession::new(binding.native_session_id()) else {
            self.fail_receiver_recovery_resume(&claimed.claim);
            return;
        };
        let Ok(capability_plan) = self.launch_capability_plan() else {
            crate::logging::log("receiver recovery failed boundary=capability-planning");
            self.fail_receiver_recovery_attempt(&claimed.claim, ReceiverRecoveryFailure::Planning);
            return;
        };
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::CapabilityPlanning,
        );

        match self.recovery_owner_decision(&claimed) {
            RecoveryOwnerDecision::Current(_) => {}
            RecoveryOwnerDecision::Lost => return,
            RecoveryOwnerDecision::StoreUnavailable => {
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoveryClaimed(claimed));
                return;
            }
        }
        let actor = claimed.claim.job().inbound().actor.clone();
        let configured_command = crate::agent::configured_command(self.context.command(), kind);
        let transport = super::launch::receiver_transport(self);
        let controller = AgentController::configured_with_command(
            self.context.command(),
            kind,
            configured_command,
            actor.clone(),
            transport,
        );
        let availability = controller.ensure_available();
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::AvailabilityProbe,
        );
        match self.recovery_owner_decision(&claimed) {
            RecoveryOwnerDecision::Current(_) => {}
            RecoveryOwnerDecision::Lost => {
                pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    claimed,
                    controller,
                    None,
                    PreSpawnRecoveryOutcome::Lost,
                );
                return;
            }
            RecoveryOwnerDecision::StoreUnavailable => {
                pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    claimed,
                    controller,
                    None,
                    PreSpawnRecoveryOutcome::RestoreClaim,
                );
                return;
            }
        }
        if availability.is_err() {
            crate::logging::log("receiver recovery failed boundary=frontend-availability");
            pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                &mut self.receiver,
                &self.services,
                claimed,
                controller,
                None,
                PreSpawnRecoveryOutcome::Failure(ReceiverRecoveryFailure::Planning),
            );
            return;
        }

        let validation = controller.resume_candidate_exists(&session);
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::ResumeValidation,
        );
        match self.recovery_owner_decision(&claimed) {
            RecoveryOwnerDecision::Current(_) => {}
            RecoveryOwnerDecision::Lost => {
                pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    claimed,
                    controller,
                    None,
                    PreSpawnRecoveryOutcome::Lost,
                );
                return;
            }
            RecoveryOwnerDecision::StoreUnavailable => {
                pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    claimed,
                    controller,
                    None,
                    PreSpawnRecoveryOutcome::RestoreClaim,
                );
                return;
            }
        }
        match validation {
            Ok(true) => {}
            Ok(false) => {
                pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    claimed,
                    controller,
                    None,
                    PreSpawnRecoveryOutcome::ResumeUnavailable,
                );
                return;
            }
            Err(_) => {
                crate::logging::log("receiver recovery failed boundary=resume-validation");
                pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                    &mut self.receiver,
                    &self.services,
                    claimed,
                    controller,
                    None,
                    PreSpawnRecoveryOutcome::ResumeUnavailable,
                );
                return;
            }
        }

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let scope = SessionScope::new(kind, self.context.workspace().id(), actor.clone());
        let spawned = {
            let registration_result = ReceiverSessionRegistration::claim_resume(
                &self.services,
                claimed.claim.job().conversation_id(),
                &claimed.identity,
                &session,
                pid,
                &scope,
            );
            #[cfg(test)]
            self.receiver.run_launch_boundary_hook(
                crate::tui::receiver::ReceiverLaunchBoundary::Registration,
            );
            let registration = match (self.recovery_owner_decision(&claimed), registration_result) {
                (RecoveryOwnerDecision::Current(_), Ok(Some(registration))) => registration,
                (RecoveryOwnerDecision::Lost, result) => {
                    let attribution = result
                        .ok()
                        .flatten()
                        .map(ReceiverSessionRegistration::commit);
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        attribution,
                        PreSpawnRecoveryOutcome::Lost,
                    );
                    return;
                }
                (RecoveryOwnerDecision::StoreUnavailable, result) => {
                    let attribution = result
                        .ok()
                        .flatten()
                        .map(ReceiverSessionRegistration::commit);
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        attribution,
                        PreSpawnRecoveryOutcome::RestoreClaim,
                    );
                    return;
                }
                (RecoveryOwnerDecision::Current(_), Ok(None)) => {
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        None,
                        PreSpawnRecoveryOutcome::Failure(ReceiverRecoveryFailure::Registration),
                    );
                    return;
                }
                (RecoveryOwnerDecision::Current(_), Err(_)) => {
                    crate::logging::log("receiver recovery failed boundary=session-registration");
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        None,
                        PreSpawnRecoveryOutcome::Failure(ReceiverRecoveryFailure::Registration),
                    );
                    return;
                }
            };

            let plan = crate::tui::receiver::planning::plan_receiver_recovery(
                claimed.claim.job().id(),
                claimed.claim.job().token(),
                session,
            );
            #[cfg(test)]
            self.receiver.run_launch_boundary_hook(
                crate::tui::receiver::ReceiverLaunchBoundary::RecoveryPreLaunchAuthorization,
            );
            let owner = match self.authorize_receiver_owner_now(&claimed.claim) {
                Ok(Some(owner)) if self.receiver.is_enabled() => owner,
                Ok(Some(_)) => {
                    let attribution = registration.commit();
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        Some(attribution),
                        PreSpawnRecoveryOutcome::RestoreClaim,
                    );
                    return;
                }
                Ok(None) => {
                    let attribution = registration.commit();
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        Some(attribution),
                        PreSpawnRecoveryOutcome::Lost,
                    );
                    return;
                }
                Err(_) => {
                    crate::logging::log(
                        "receiver recovery deferred boundary=pre-launch-owner-store",
                    );
                    let attribution = registration.commit();
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        Some(attribution),
                        PreSpawnRecoveryOutcome::RestoreClaim,
                    );
                    return;
                }
            };
            #[cfg(test)]
            self.receiver.run_launch_boundary_hook(
                crate::tui::receiver::ReceiverLaunchBoundary::RecoveryLaunchPreparation,
            );
            match self.services.prepare_receiver_recovery_launch(
                claimed.claim.job().id(),
                claimed.claim.claim().owner(),
                owner.observed_at_unix_ms(),
            ) {
                Ok(true) => {}
                Ok(false) => {
                    let attribution = registration.commit();
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        Some(attribution),
                        PreSpawnRecoveryOutcome::Lost,
                    );
                    return;
                }
                Err(_) => {
                    crate::logging::log(
                        "receiver recovery failed boundary=launch-preparation-store",
                    );
                    let attribution = registration.commit();
                    pre_spawn_cleanup::begin_recovery_pre_spawn_cleanup(
                        &mut self.receiver,
                        &self.services,
                        claimed,
                        controller,
                        Some(attribution),
                        PreSpawnRecoveryOutcome::RestoreClaim,
                    );
                    return;
                }
            }

            let hooks = self.receiver_hook_metadata(&claimed, pid);
            let mut request = LaunchRequest::from_trusted_context(
                Arc::clone(&self.context.command().workspace),
                actor,
                plan.session_plan().clone(),
                Some(plan.initial_prompt().to_owned()),
                self.context.access_mode(),
            );
            if let Some(capability_plan) = capability_plan {
                request = request.with_capability_plan(capability_plan);
            }
            request = request.with_hook_metadata(hooks);
            effects::spawn_claimed_receiver_recovery(
                &self.services,
                claimed,
                registration,
                controller,
                &request,
                pid,
                || {
                    #[cfg(test)]
                    self.receiver.run_launch_boundary_hook(
                        crate::tui::receiver::ReceiverLaunchBoundary::Spawn,
                    );
                },
            )
        };
        if let Some(spawned) = spawned {
            self.continue_spawned_receiver_recovery(spawned);
        }
    }

    fn recovery_owner_decision(&self, claimed: &ClaimedReceiverRun) -> RecoveryOwnerDecision {
        match self.authorize_receiver_owner_now(&claimed.claim) {
            Ok(Some(owner)) => RecoveryOwnerDecision::Current(owner),
            Ok(None) => RecoveryOwnerDecision::Lost,
            Err(_) => {
                crate::logging::log("receiver recovery deferred boundary=owner-store");
                RecoveryOwnerDecision::StoreUnavailable
            }
        }
    }

    fn fail_receiver_recovery_resume(&self, claim: &ReceiverRunClaim) {
        let now = self.receiver_now_unix_ms();
        if self
            .services
            .fail_receiver_recovery_resume(claim.job().id(), claim.claim().owner(), now)
            .is_err()
        {
            crate::logging::log("receiver recovery failed boundary=resume-failure-store");
        }
    }

    fn fail_receiver_recovery_attempt(
        &self,
        claim: &ReceiverRunClaim,
        failure: ReceiverRecoveryFailure,
    ) {
        let now = self.receiver_now_unix_ms();
        if self
            .services
            .fail_receiver_recovery_attempt(claim.job().id(), claim.claim().owner(), now, failure)
            .is_err()
        {
            crate::logging::log("receiver recovery failed boundary=launch-failure-store");
        }
    }
}

pub(super) fn shutdown_failure_or(
    cleanup: &anyhow::Result<()>,
    otherwise: ReceiverRecoveryFailure,
) -> ReceiverRecoveryFailure {
    if cleanup.is_err() {
        ReceiverRecoveryFailure::Shutdown
    } else {
        otherwise
    }
}
