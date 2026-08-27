//! Same-session launch of one durably claimed receiver recovery.

use std::sync::Arc;

use crate::agent::{AgentController, AgentSession, LaunchRequest, SessionScope};
use crate::state::{ReceiverRecoveryFailure, ReceiverRunClaim};
use crate::tui::App;
use crate::tui::receiver::{
    ClaimedReceiverRun, DurableReceiverRun, ReceiverRemoteSession, ReceiverSessionRegistration,
    cleanup_receiver_launch,
};

mod effects;

impl App {
    pub(super) fn claim_receiver_recovery_run(&mut self) -> bool {
        if !self.brain.receiver_run_observations().is_empty() {
            return true;
        }
        let remote = ReceiverRemoteSession::new(self.brain.instance());
        let now = self.receiver_now_unix_ms();
        match self.services.claim_receiver_recovery_run(
            remote.instance(),
            now,
            now.saturating_add(super::dispatch::CLAIM_LIFETIME_MS),
        ) {
            Ok(Some(claim)) => {
                self.launch_claimed_receiver_recovery(ClaimedReceiverRun {
                    claim,
                    remote,
                    freshness_ready: true,
                });
                true
            }
            Ok(None) => false,
            Err(_) => {
                crate::logging::log("receiver recovery failed boundary=claim-store");
                true
            }
        }
    }

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

        if !self.recovery_owner_is_current(&claimed) {
            return;
        }
        let actor = claimed.claim.job().inbound().actor.clone();
        let configured_command = crate::agent::configured_command(self.context.command(), kind);
        let transport = super::launch::receiver_transport(self);
        let mut controller = AgentController::configured_with_command(
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
        if !self.recovery_owner_is_current(&claimed) {
            let _ =
                cleanup_receiver_launch::<crate::tui::state::AppServices>(None, &mut controller);
            return;
        }
        if availability.is_err() {
            crate::logging::log("receiver recovery failed boundary=frontend-availability");
            let failure = shutdown_failure_or(
                &cleanup_receiver_launch::<crate::tui::state::AppServices>(None, &mut controller),
                ReceiverRecoveryFailure::Planning,
            );
            self.fail_receiver_recovery_attempt(&claimed.claim, failure);
            return;
        }

        let validation = controller.resume_candidate_exists(&session);
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::ResumeValidation,
        );
        if !self.recovery_owner_is_current(&claimed) {
            let _ =
                cleanup_receiver_launch::<crate::tui::state::AppServices>(None, &mut controller);
            return;
        }
        match validation {
            Ok(true) => {}
            Ok(false) => {
                let cleanup = cleanup_receiver_launch::<crate::tui::state::AppServices>(
                    None,
                    &mut controller,
                );
                if cleanup.is_err() {
                    self.fail_receiver_recovery_attempt(
                        &claimed.claim,
                        ReceiverRecoveryFailure::Shutdown,
                    );
                } else {
                    self.fail_receiver_recovery_resume(&claimed.claim);
                }
                return;
            }
            Err(_) => {
                crate::logging::log("receiver recovery failed boundary=resume-validation");
                let cleanup = cleanup_receiver_launch::<crate::tui::state::AppServices>(
                    None,
                    &mut controller,
                );
                if cleanup.is_err() {
                    self.fail_receiver_recovery_attempt(
                        &claimed.claim,
                        ReceiverRecoveryFailure::Shutdown,
                    );
                } else {
                    self.fail_receiver_recovery_resume(&claimed.claim);
                }
                return;
            }
        }

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let scope = SessionScope::new(kind, self.context.workspace().id(), actor.clone());
        let registration = ReceiverSessionRegistration::claim_resume(
            &self.services,
            claimed.claim.job().conversation_id(),
            &claimed.remote,
            &session,
            pid,
            &scope,
        );
        #[cfg(test)]
        self.receiver
            .run_launch_boundary_hook(crate::tui::receiver::ReceiverLaunchBoundary::Registration);
        if !self.recovery_owner_is_current(&claimed) {
            if let Ok(Some(registration)) = registration {
                let _ = registration.cleanup();
            }
            let _ = controller.shutdown();
            return;
        }
        let registration = match registration {
            Ok(Some(registration)) => registration,
            Ok(None) => {
                let failure = shutdown_failure_or(
                    &cleanup_receiver_launch::<crate::tui::state::AppServices>(
                        None,
                        &mut controller,
                    ),
                    ReceiverRecoveryFailure::Registration,
                );
                self.fail_receiver_recovery_attempt(&claimed.claim, failure);
                return;
            }
            Err(_) => {
                crate::logging::log("receiver recovery failed boundary=session-registration");
                let failure = shutdown_failure_or(
                    &cleanup_receiver_launch::<crate::tui::state::AppServices>(
                        None,
                        &mut controller,
                    ),
                    ReceiverRecoveryFailure::Registration,
                );
                self.fail_receiver_recovery_attempt(&claimed.claim, failure);
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
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoveryClaimed(claimed));
                return;
            }
            Ok(None) => {
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
                return;
            }
            Err(_) => {
                crate::logging::log("receiver recovery deferred boundary=pre-launch-owner-store");
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
                self.receiver
                    .store_durable_run(DurableReceiverRun::RecoveryClaimed(claimed));
                return;
            }
        };
        match self.services.prepare_receiver_recovery_launch(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            owner.observed_at_unix_ms(),
        ) {
            Ok(true) => {}
            Ok(false) => {
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
                return;
            }
            Err(_) => {
                crate::logging::log("receiver recovery failed boundary=launch-preparation-store");
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
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
            &mut self.brain,
            &mut self.receiver,
            &self.services,
            claimed,
            registration,
            controller,
            &request,
        );
    }

    fn recovery_owner_is_current(&self, claimed: &ClaimedReceiverRun) -> bool {
        match self.authorize_receiver_owner_now(&claimed.claim) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => {
                crate::logging::log("receiver recovery deferred boundary=owner-store");
                false
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
