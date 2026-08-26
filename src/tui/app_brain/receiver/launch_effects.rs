//! Process spawn and background receiver-tab allocation effects.

use crate::agent::{AgentController, LaunchRequest};
use crate::state::ReceiverLaunchFailure;
use crate::tui::receiver::attachments::PreparedReceiverAttachments;
use crate::tui::receiver::{
    ClaimedReceiverRun, DurableReceiverRun, ReceiverRuntime, ReceiverSessionRegistration,
    cleanup_receiver_launch,
};
use crate::tui::state::{AppServices, BrainPanelState};

pub(super) struct ReceiverLaunchEffects<'app> {
    brain: &'app mut BrainPanelState,
    receiver: &'app mut ReceiverRuntime,
    services: &'app AppServices,
}

impl<'app> ReceiverLaunchEffects<'app> {
    pub(super) fn new(
        brain: &'app mut BrainPanelState,
        receiver: &'app mut ReceiverRuntime,
        services: &'app AppServices,
    ) -> Self {
        Self {
            brain,
            receiver,
            services,
        }
    }

    pub(super) fn spawn(
        self,
        claimed: ClaimedReceiverRun,
        staged_attachments: PreparedReceiverAttachments,
        registration: ReceiverSessionRegistration<'_, AppServices>,
        mut controller: AgentController,
        request: &LaunchRequest,
    ) {
        let Self {
            brain,
            receiver,
            services,
        } = self;
        let launch = controller.launch(request);
        #[cfg(test)]
        receiver.run_launch_boundary_hook(crate::tui::receiver::ReceiverLaunchBoundary::Spawn);
        if let Err(error) = launch {
            crate::logging::log(format!("receiver process spawn failed: {error}"));
            let _ = cleanup_receiver_launch(Some(registration), &mut controller);
            let _ = super::ownership::retry_receiver_owner_now(
                services,
                &claimed.claim,
                ReceiverLaunchFailure::Spawn,
            );
            return;
        }
        let owner = match super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
            Ok(Some(owner)) => owner,
            Ok(None) => {
                preserve_successful_spawn(registration, &mut controller);
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver post-spawn claim validation failed: {error:#}"
                ));
                preserve_successful_spawn(registration, &mut controller);
                return;
            }
        };
        let attribution = registration.attribution();
        let launch_observation = crate::state::ReceiverLaunchObservation {
            token: claimed.claim.job().token(),
            instance: attribution.instance().to_owned(),
            session_id: attribution.registered_session().as_str().to_owned(),
            observed_at_unix_ms: owner.observed_at_unix_ms(),
            authorized_at_unix_ms: owner.observed_at_unix_ms(),
        };
        match services.commit_receiver_job_launch(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            &launch_observation,
        ) {
            Ok(true) => {}
            Ok(false) => {
                preserve_successful_spawn(registration, &mut controller);
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver launch observation commit failed: {error:#}"
                ));
                preserve_successful_spawn(registration, &mut controller);
                return;
            }
        }

        let title = format!(
            "Receiver · {}",
            match claimed.claim.job().inbound().channel {
                crate::server::receiver::Channel::Sms => "SMS",
                crate::server::receiver::Channel::Email => "Email",
            }
        );
        let tab = brain.add_receiver_run(
            claimed.claim.job().id(),
            title,
            claimed.remote.instance().to_owned(),
            controller,
        );
        #[cfg(test)]
        receiver.run_launch_boundary_hook(crate::tui::receiver::ReceiverLaunchBoundary::Allocation);
        let tab_id = match tab {
            Ok(tab_id) => tab_id,
            Err(error) => {
                crate::logging::log(format!("receiver tab allocation failed: {error}"));
                let _ = registration.commit();
                return;
            }
        };
        match super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = brain.remove_receiver_run(tab_id);
                let _ = registration.commit();
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver post-allocation claim validation failed: {error:#}"
                ));
                let _ = brain.remove_receiver_run(tab_id);
                let _ = registration.commit();
                return;
            }
        }
        let attribution = registration.commit();
        receiver.store_durable_run(DurableReceiverRun::Active(
            crate::tui::receiver::ActiveReceiverRun {
                claim: claimed.claim,
                attribution,
                tab_id,
                _attachments: staged_attachments,
            },
        ));
    }
}

fn preserve_successful_spawn(
    registration: ReceiverSessionRegistration<'_, AppServices>,
    controller: &mut AgentController,
) {
    let _ = registration.commit();
    let _ = controller.shutdown();
}
