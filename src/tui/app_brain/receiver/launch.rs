//! Claim-authorized planning and session registration before process launch.

use std::sync::Arc;

use crate::agent::{AgentController, HookMetadata, LaunchRequest};
use crate::pty_pane::PtyPane;
use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::attachments::PreparedReceiverAttachments;
use crate::tui::receiver::{
    ClaimedReceiverRun, DurableReceiverRun, ReceiverSessionRegistration, cleanup_receiver_launch,
};
use crate::tui::state::AppServices;

#[cfg(not(test))]
fn receiver_transport(_app: &mut App) -> Box<dyn crate::agent::AgentTransport> {
    Box::new(PtyPane::new(24, 80))
}

#[cfg(test)]
fn receiver_transport(app: &mut App) -> Box<dyn crate::agent::AgentTransport> {
    app.brain
        .take_receiver_transport()
        .unwrap_or_else(|| Box::new(PtyPane::new(24, 80)))
}

fn cleanup_unregistered(controller: &mut AgentController) {
    let _ = cleanup_receiver_launch(
        None::<ReceiverSessionRegistration<'_, AppServices>>,
        controller,
    );
}

impl App {
    pub(super) fn launch_claimed_receiver_run_with_attachments(
        &mut self,
        claimed: ClaimedReceiverRun,
        staged_attachments: PreparedReceiverAttachments,
    ) {
        let staged_attachment_work = !staged_attachments.staged().is_empty();
        let local_attachment_paths = staged_attachments
            .staged()
            .iter()
            .map(|attachment| attachment.path.as_deref())
            .collect::<Option<Vec<_>>>();
        let Some(local_attachment_paths) = local_attachment_paths
            .filter(|paths| paths.len() == claimed.claim.job().inbound().attachments.len())
        else {
            crate::logging::log("receiver attachment prompt preparation failed");
            let _ = self.retry_receiver_owner_now(&claimed.claim, ReceiverLaunchFailure::Planning);
            return;
        };
        let capability_plan = self.launch_capability_plan();
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::CapabilityPlanning,
        );
        match self.authorize_receiver_owner_now(&claimed.claim) {
            Ok(Some(_)) => {}
            Ok(None) => return,
            Err(error) => {
                crate::logging::log(format!(
                    "receiver post-capability claim validation failed: {error:#}"
                ));
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
        }
        let capability_plan = match capability_plan {
            Ok(plan) => plan,
            Err(error) => {
                crate::logging::log(format!(
                    "receiver launch capability planning failed: {error:#}"
                ));
                let _ =
                    self.retry_receiver_owner_now(&claimed.claim, ReceiverLaunchFailure::Planning);
                return;
            }
        };

        let transport = receiver_transport(self);
        let actor = claimed.claim.job().inbound().actor.clone();
        let mut controller = self.controller_for_transport(actor.clone(), transport);
        let availability = controller.ensure_available();
        #[cfg(test)]
        self.receiver.run_launch_boundary_hook(
            crate::tui::receiver::ReceiverLaunchBoundary::AvailabilityProbe,
        );
        match self.authorize_receiver_owner_now(&claimed.claim) {
            Ok(Some(_)) => {}
            Ok(None) => {
                cleanup_unregistered(&mut controller);
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver post-availability claim validation failed: {error:#}"
                ));
                cleanup_unregistered(&mut controller);
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
        }
        if let Err(error) = availability {
            crate::logging::log(format!("receiver frontend unavailable: {error}"));
            cleanup_unregistered(&mut controller);
            let _ = self.retry_receiver_owner_now(&claimed.claim, ReceiverLaunchFailure::Planning);
            return;
        }

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let scope = crate::agent::SessionScope::new(
            self.context.agent_kind(),
            self.context.workspace().id(),
            actor.clone(),
        );
        #[cfg(test)]
        let receiver = &mut self.receiver;
        let resume = super::resume::decide(
            &controller,
            &self.services,
            &claimed,
            pid,
            &scope,
            |boundary| {
                #[cfg(test)]
                receiver.run_launch_boundary_hook(match boundary {
                    super::resume::ReceiverResumeBoundary::Validation => {
                        crate::tui::receiver::ReceiverLaunchBoundary::ResumeValidation
                    }
                    super::resume::ReceiverResumeBoundary::Registration => {
                        crate::tui::receiver::ReceiverLaunchBoundary::Registration
                    }
                });
                #[cfg(not(test))]
                let _ = boundary;
            },
        );
        let (resume_session, resume_registration) = match resume {
            super::resume::ReceiverResumeDecision::Fresh => (None, None),
            super::resume::ReceiverResumeDecision::Registered {
                session,
                registration,
            } => (Some(session), Some(registration)),
            super::resume::ReceiverResumeDecision::Lost => {
                cleanup_unregistered(&mut controller);
                return;
            }
            super::resume::ReceiverResumeDecision::Deferred => {
                cleanup_unregistered(&mut controller);
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
            super::resume::ReceiverResumeDecision::RegistrationFailed => {
                cleanup_unregistered(&mut controller);
                let _ = self
                    .retry_receiver_owner_now(&claimed.claim, ReceiverLaunchFailure::Registration);
                return;
            }
        };
        let Some(plan) = crate::tui::receiver::planning::plan_receiver_launch(
            claimed.claim.job(),
            claimed.claim.conversation(),
            &local_attachment_paths,
            claimed.remote.placeholder().clone(),
            resume_session,
        ) else {
            if let Some(registration) = resume_registration {
                let _ = registration.cleanup();
            }
            cleanup_unregistered(&mut controller);
            let _ = self.retry_receiver_owner_now(&claimed.claim, ReceiverLaunchFailure::Planning);
            return;
        };

        let registration = if let Some(registration) = resume_registration {
            registration
        } else {
            let registration = ReceiverSessionRegistration::register_fresh(
                &self.services,
                claimed.claim.job().conversation_id(),
                &claimed.remote,
                pid,
                &scope,
            );
            #[cfg(test)]
            self.receiver.run_launch_boundary_hook(
                crate::tui::receiver::ReceiverLaunchBoundary::Registration,
            );
            match self.authorize_receiver_owner_now(&claimed.claim) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    cleanup_unregistered(&mut controller);
                    return;
                }
                Err(error) => {
                    crate::logging::log(format!(
                        "receiver post-registration claim validation failed: {error:#}"
                    ));
                    cleanup_unregistered(&mut controller);
                    self.receiver
                        .store_durable_run(DurableReceiverRun::Claimed(claimed));
                    return;
                }
            }
            match registration {
                Ok(registration) => registration,
                Err(error) => {
                    crate::logging::log(format!("receiver session registration failed: {error:#}"));
                    cleanup_unregistered(&mut controller);
                    let _ = self.retry_receiver_owner_now(
                        &claimed.claim,
                        ReceiverLaunchFailure::Registration,
                    );
                    return;
                }
            }
        };

        let owner = match self.authorize_receiver_owner_now(&claimed.claim) {
            Ok(Some(owner)) if self.receiver.is_enabled() || !staged_attachment_work => owner,
            Ok(Some(_)) => {
                let _ = registration.cleanup();
                let _ = controller.shutdown();
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
            Ok(None) => {
                let _ = registration.cleanup();
                let _ = controller.shutdown();
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver pre-launch claim validation failed: {error:#}"
                ));
                let _ = registration.cleanup();
                let _ = controller.shutdown();
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
        };
        match self.services.prepare_receiver_launch(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            owner.observed_at_unix_ms(),
        ) {
            Ok(true) => {}
            Ok(false) => {
                let _ = registration.cleanup();
                let _ = controller.shutdown();
                return;
            }
            Err(error) => {
                crate::logging::log(format!("receiver launch preparation failed: {error:#}"));
                let _ = registration.cleanup();
                let _ = controller.shutdown();
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
        super::launch_effects::ReceiverLaunchEffects::new(
            &mut self.brain,
            &mut self.receiver,
            &self.services,
        )
        .spawn(
            claimed,
            staged_attachments,
            registration,
            controller,
            &request,
        );
    }

    fn receiver_hook_metadata(&self, claimed: &ClaimedReceiverRun, pid: i32) -> HookMetadata {
        HookMetadata::new(vec![
            (
                "BRAIN_INSTANCE_ID".to_owned(),
                claimed.remote.instance().to_owned(),
            ),
            ("BRAIN_PID".to_owned(), pid.to_string()),
            (
                "BRAIN_STATE_DB".to_owned(),
                self.context.state_db_path().display().to_string(),
            ),
            (
                "BRAIN_RESPONSE_ID".to_owned(),
                claimed.remote.instance().to_owned(),
            ),
            (
                "BRAIN_RESPONSE_DIR".to_owned(),
                self.context
                    .workspace()
                    .paths()
                    .responses_dir()
                    .display()
                    .to_string(),
            ),
            (
                "BRAIN_RECEIVER_JOB_TOKEN".to_owned(),
                claimed.claim.job().token().to_string(),
            ),
            (
                "BRAIN_RECEIVER_OBSERVATION_PATH".to_owned(),
                self.context
                    .workspace()
                    .paths()
                    .receiver_observations_dir()
                    .join(format!("{}.json", claimed.remote.instance()))
                    .display()
                    .to_string(),
            ),
        ])
    }
}
