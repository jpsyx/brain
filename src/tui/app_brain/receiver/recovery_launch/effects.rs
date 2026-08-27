//! Post-registration recovery spawn and durable launch effects.

use crate::agent::{AgentController, LaunchRequest};
use crate::state::{ReceiverRecoveryFailure, ReceiverRunClaim};
use crate::tui::receiver::attachments::PreparedReceiverAttachments;
use crate::tui::receiver::{
    ActiveReceiverRun, ClaimedReceiverRun, DurableReceiverRun, ReceiverSessionRegistration,
    cleanup_receiver_launch,
};

pub(super) fn spawn_claimed_receiver_recovery(
    brain: &mut crate::tui::state::BrainPanelState,
    receiver: &mut crate::tui::receiver::ReceiverRuntime,
    services: &crate::tui::state::AppServices,
    claimed: ClaimedReceiverRun,
    registration: ReceiverSessionRegistration<'_, crate::tui::state::AppServices>,
    mut controller: AgentController,
    request: &LaunchRequest,
) {
    let launch = controller.launch(request);
    #[cfg(test)]
    receiver.run_launch_boundary_hook(crate::tui::receiver::ReceiverLaunchBoundary::Spawn);
    if launch.is_err() {
        crate::logging::log("receiver recovery failed boundary=process-spawn");
        let failure = super::shutdown_failure_or(
            &cleanup_receiver_launch(Some(registration), &mut controller),
            ReceiverRecoveryFailure::Spawn,
        );
        fail_receiver_recovery_attempt(services, &claimed.claim, failure);
        return;
    }
    let owner =
        match super::super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
            Ok(Some(owner)) => owner,
            Ok(None) => {
                preserve_recovery_spawn(registration, &mut controller);
                return;
            }
            Err(_) => {
                crate::logging::log("receiver recovery deferred boundary=post-spawn-owner-store");
                preserve_recovery_spawn(registration, &mut controller);
                return;
            }
        };
    let attribution = registration.attribution();
    let observation = crate::state::ReceiverLaunchObservation {
        token: claimed.claim.job().token(),
        instance: attribution.instance().to_owned(),
        session_id: attribution.registered_session().as_str().to_owned(),
        observed_at_unix_ms: owner.observed_at_unix_ms(),
        authorized_at_unix_ms: owner.observed_at_unix_ms(),
    };
    match services.commit_receiver_job_launch(
        claimed.claim.job().id(),
        claimed.claim.claim().owner(),
        &observation,
    ) {
        Ok(true) => {}
        Ok(false) => {
            preserve_recovery_spawn(registration, &mut controller);
            return;
        }
        Err(_) => {
            crate::logging::log("receiver recovery deferred boundary=launch-commit-store");
            preserve_recovery_spawn(registration, &mut controller);
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
    let Ok(tab_id) = brain.add_receiver_run(
        claimed.claim.job().id(),
        title,
        claimed.remote.instance().to_owned(),
        controller,
    ) else {
        crate::logging::log("receiver recovery failed boundary=tab-allocation");
        let _ = registration.commit();
        return;
    };
    #[cfg(test)]
    receiver.run_launch_boundary_hook(crate::tui::receiver::ReceiverLaunchBoundary::Allocation);
    match super::super::ownership::authorize_receiver_owner_now(services, &claimed.claim) {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = brain.remove_receiver_run(tab_id);
            let _ = registration.commit();
            return;
        }
        Err(_) => {
            crate::logging::log("receiver recovery deferred boundary=post-allocation-owner-store");
            let _ = brain.remove_receiver_run(tab_id);
            let _ = registration.commit();
            return;
        }
    }
    let attribution = registration.commit();
    receiver.store_durable_run(DurableReceiverRun::Active(ActiveReceiverRun {
        claim: claimed.claim,
        attribution,
        tab_id,
        _attachments: PreparedReceiverAttachments::empty(),
    }));
}

fn preserve_recovery_spawn(
    registration: ReceiverSessionRegistration<'_, crate::tui::state::AppServices>,
    controller: &mut AgentController,
) {
    let _ = registration.commit();
    let _ = controller.shutdown();
}

fn fail_receiver_recovery_attempt(
    services: &crate::tui::state::AppServices,
    claim: &ReceiverRunClaim,
    failure: ReceiverRecoveryFailure,
) {
    let now = u64::try_from(services.utc_now().timestamp_millis()).unwrap_or(0);
    if services
        .fail_receiver_recovery_attempt(claim.job().id(), claim.claim().owner(), now, failure)
        .is_err()
    {
        crate::logging::log("receiver recovery failed boundary=launch-failure-store");
    }
}
