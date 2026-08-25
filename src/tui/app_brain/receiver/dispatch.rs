//! Durable receiver-run coordination from the application event loop.

use std::sync::Arc;

use crate::agent::{HookMetadata, LaunchRequest, SessionPlan};
use crate::pty_pane::PtyPane;
use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::{
    ClaimedReceiverRun, DurableReceiverRun, ReceiverRemoteSession, ReceiverSessionRegistration,
    rollback_receiver_launch,
};

pub(super) const CLAIM_LIFETIME_MS: u64 = 30_000;
pub(super) const RETRY_DELAY_MS: u64 = 5_000;

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

impl App {
    /// Advance the single durable receiver consumer by one non-blocking step.
    pub(crate) fn tick_receiver(&mut self) {
        let receiver_enabled = self.receiver.is_enabled();
        if receiver_enabled {
            self.apply_receiver_restarts();
            #[cfg(test)]
            self.receiver.run_after_restart_scan_hook();
        }
        match self.receiver.take_durable_run() {
            DurableReceiverRun::Active(active) => self.tick_active_receiver_run(active),
            DurableReceiverRun::Claimed(claimed) => self.continue_claimed_receiver_run(claimed),
            DurableReceiverRun::Idle if receiver_enabled => self.claim_receiver_run(),
            DurableReceiverRun::Idle => {}
        }
    }

    pub(super) fn claim_receiver_run(&mut self) {
        if !self.brain.receiver_run_observations().is_empty() {
            return;
        }
        let remote = ReceiverRemoteSession::new(self.brain.instance());
        let now = self.receiver_now_unix_ms();
        match self.services.claim_receiver_run(
            remote.instance(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(Some(claim)) => {
                self.continue_claimed_receiver_run(ClaimedReceiverRun { claim, remote });
            }
            Ok(None) => {}
            Err(error) => crate::logging::log(format!("durable receiver claim failed: {error:#}")),
        }
    }

    fn continue_claimed_receiver_run(&mut self, claimed: ClaimedReceiverRun) {
        let now = self.receiver_now_unix_ms();
        match self.services.renew_receiver_claim(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                crate::logging::log(format!("receiver pending claim renewal failed: {error:#}"));
                self.receiver
                    .store_durable_run(DurableReceiverRun::Claimed(claimed));
                return;
            }
        }
        if self.execute_receiver_sync_freshness_effect()
            == crate::tui::receiver::ReceiverEffectOutcome::FreshnessPending
        {
            self.receiver
                .store_durable_run(DurableReceiverRun::Claimed(claimed));
            return;
        }
        if crate::server::receiver::parse_control_command(&claimed.claim.job().inbound().prompt)
            == Some(crate::server::receiver::ControlCommand::NewSession)
        {
            self.complete_receiver_new_session(claimed);
            return;
        }
        self.launch_claimed_receiver_run(claimed);
    }

    fn launch_claimed_receiver_run(&mut self, claimed: ClaimedReceiverRun) {
        let now = self.receiver_now_unix_ms();
        let retry_at = now.saturating_add(RETRY_DELAY_MS);
        let Ok(staged_attachments) = self.services.stage_receiver_attachments(
            self.context.workspace(),
            self.context.command(),
            claimed.claim.job().inbound(),
        ) else {
            crate::logging::log("receiver attachment preparation failed");
            self.retry_unregistered_receiver(&claimed, ReceiverLaunchFailure::Planning, now);
            return;
        };
        if staged_attachments
            .iter()
            .any(|attachment| attachment.path.is_none() || attachment.error.is_some())
        {
            crate::logging::log("receiver attachment preparation failed");
            self.retry_unregistered_receiver(&claimed, ReceiverLaunchFailure::Planning, now);
            return;
        }
        let capability_plan = match self.launch_capability_plan() {
            Ok(plan) => plan,
            Err(error) => {
                crate::logging::log(format!(
                    "receiver launch capability planning failed: {error:#}"
                ));
                self.retry_unregistered_receiver(&claimed, ReceiverLaunchFailure::Planning, now);
                return;
            }
        };
        let transport = receiver_transport(self);
        let actor = claimed.claim.job().inbound().actor.clone();
        let mut controller = self.controller_for_transport(actor.clone(), transport);
        if let Err(error) = controller.ensure_available() {
            crate::logging::log(format!("receiver frontend unavailable: {error}"));
            let _ = rollback_receiver_launch(
                &self.services,
                &claimed.claim,
                None::<ReceiverSessionRegistration<'_, crate::tui::state::AppServices>>,
                &mut controller,
                ReceiverLaunchFailure::Planning,
                now,
                retry_at,
            );
            return;
        }

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let scope = crate::agent::SessionScope::new(
            self.context.agent_kind(),
            self.context.workspace().id(),
            actor.clone(),
        );
        let mut resume_registration = None;
        let plan = crate::tui::receiver::planning::plan_receiver_launch(
            &controller,
            claimed.claim.job(),
            claimed.claim.conversation(),
            claimed.remote.placeholder().clone(),
            |session| {
                let registration = ReceiverSessionRegistration::claim_resume(
                    &self.services,
                    claimed.claim.job().conversation_id(),
                    &claimed.remote,
                    session,
                    pid,
                    &scope,
                )?;
                let was_claimed = registration.is_some();
                resume_registration = registration;
                Ok(was_claimed)
            },
        );
        let registration = if matches!(plan.session_plan(), SessionPlan::Fresh(_)) {
            match ReceiverSessionRegistration::register_fresh(
                &self.services,
                claimed.claim.job().conversation_id(),
                &claimed.remote,
                pid,
                &scope,
            ) {
                Ok(registration) => registration,
                Err(error) => {
                    crate::logging::log(format!("receiver session registration failed: {error:#}"));
                    let _ = rollback_receiver_launch(
                        &self.services,
                        &claimed.claim,
                        None::<ReceiverSessionRegistration<'_, crate::tui::state::AppServices>>,
                        &mut controller,
                        ReceiverLaunchFailure::Registration,
                        now,
                        retry_at,
                    );
                    return;
                }
            }
        } else {
            let Some(registration) = resume_registration else {
                let _ = rollback_receiver_launch(
                    &self.services,
                    &claimed.claim,
                    None::<ReceiverSessionRegistration<'_, crate::tui::state::AppServices>>,
                    &mut controller,
                    ReceiverLaunchFailure::Registration,
                    now,
                    retry_at,
                );
                return;
            };
            registration
        };

        match self.services.prepare_receiver_launch(
            claimed.claim.job().id(),
            claimed.claim.claim().owner(),
            now,
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

        let Some(initial_prompt) =
            localize_attachment_references(plan.initial_prompt(), &staged_attachments)
        else {
            crate::logging::log("receiver attachment prompt preparation failed");
            let _ = rollback_receiver_launch(
                &self.services,
                &claimed.claim,
                Some(registration),
                &mut controller,
                ReceiverLaunchFailure::Planning,
                now,
                retry_at,
            );
            return;
        };
        let hooks = self.receiver_hook_metadata(&claimed, pid);
        let mut request = LaunchRequest::from_trusted_context(
            Arc::clone(&self.context.command().workspace),
            actor,
            plan.session_plan().clone(),
            Some(initial_prompt),
            self.context.access_mode(),
        );
        if let Some(capability_plan) = capability_plan {
            request = request.with_capability_plan(capability_plan);
        }
        request = request.with_hook_metadata(hooks);
        if let Err(error) = controller.launch(&request) {
            crate::logging::log(format!("receiver process spawn failed: {error}"));
            let _ = rollback_receiver_launch(
                &self.services,
                &claimed.claim,
                Some(registration),
                &mut controller,
                ReceiverLaunchFailure::Spawn,
                now,
                retry_at,
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
        let tab_id = match self.brain.add_receiver_run(
            claimed.claim.job().id(),
            title,
            claimed.remote.instance().to_owned(),
            controller,
        ) {
            Ok(tab_id) => tab_id,
            Err(error) => {
                crate::logging::log(format!("receiver tab allocation failed: {error}"));
                let _ = registration.cleanup();
                let _ = self.services.record_receiver_launch_retry(
                    claimed.claim.job().id(),
                    claimed.claim.claim().owner(),
                    now,
                    retry_at,
                    ReceiverLaunchFailure::Allocation,
                );
                return;
            }
        };
        let attribution = registration.commit();
        self.receiver.store_durable_run(DurableReceiverRun::Active(
            crate::tui::receiver::ActiveReceiverRun {
                claim: claimed.claim,
                attribution,
                tab_id,
            },
        ));
    }

    fn retry_unregistered_receiver(
        &mut self,
        claimed: &ClaimedReceiverRun,
        failure: ReceiverLaunchFailure,
        now: u64,
    ) {
        let actor = claimed.claim.job().inbound().actor.clone();
        let transport = receiver_transport(self);
        let mut controller = self.controller_for_transport(actor, transport);
        let _ = rollback_receiver_launch(
            &self.services,
            &claimed.claim,
            None::<ReceiverSessionRegistration<'_, crate::tui::state::AppServices>>,
            &mut controller,
            failure,
            now,
            now.saturating_add(RETRY_DELAY_MS),
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
        ])
    }

    pub(super) fn receiver_now_unix_ms(&self) -> u64 {
        u64::try_from(self.services.utc_now().timestamp_millis()).unwrap_or(0)
    }
}

fn localize_attachment_references(
    prompt: &str,
    attachments: &[crate::server::receiver::StagedAttachment],
) -> Option<String> {
    if attachments.is_empty() {
        return Some(prompt.to_owned());
    }
    let marker = "\n\nAttachment references:";
    let start = prompt.rfind(marker)?;
    let mut localized = prompt[..start].to_owned();
    localized.push_str("\n\nLocal attachment files:");
    for attachment in attachments {
        use std::fmt::Write as _;

        let path = attachment.path.as_ref()?;
        let encoded = serde_json::to_string(&path.display().to_string()).ok()?;
        let _ = write!(localized, "\n- path={encoded}");
    }
    Some(localized)
}
