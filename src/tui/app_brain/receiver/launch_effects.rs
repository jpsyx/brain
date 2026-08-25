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
        match super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver post-spawn claim validation failed: {error:#}"
                ));
                let _ = cleanup_receiver_launch(Some(registration), &mut controller);
                return;
            }
        }
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
        match super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
            Ok(Some(_)) => {}
            Ok(None) => {
                remove_new_receiver_tab(brain, &tab);
                let _ = registration.cleanup();
                return;
            }
            Err(error) => {
                crate::logging::log(format!(
                    "receiver post-allocation claim validation failed: {error:#}"
                ));
                remove_new_receiver_tab(brain, &tab);
                let _ = registration.cleanup();
                return;
            }
        }
        let tab_id = match tab {
            Ok(tab_id) => tab_id,
            Err(error) => {
                crate::logging::log(format!("receiver tab allocation failed: {error}"));
                let _ = registration.cleanup();
                let _ = super::ownership::retry_receiver_owner_now(
                    services,
                    &claimed.claim,
                    ReceiverLaunchFailure::Allocation,
                );
                return;
            }
        };
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

fn remove_new_receiver_tab<Error>(
    brain: &mut BrainPanelState,
    tab: &Result<crate::tui::model::SessionTabId, Error>,
) {
    if let Ok(tab_id) = tab {
        let _ = brain.remove_receiver_run(*tab_id);
    }
}
