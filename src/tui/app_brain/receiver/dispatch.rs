//! Ordered receiver decisions and application-owned effect execution.

use crate::server::receiver::InboundJob;
use crate::tui::*;

impl App {
    /// Plan and execute one pass over the receiver's ordered lifecycle stages.
    pub(crate) fn tick_receiver(&mut self) {
        crate::tui::receiver::run_receiver_tick(|stage| self.execute_receiver_tick_stage(stage));
    }

    fn execute_receiver_tick_stage(
        &mut self,
        stage: crate::tui::receiver::TickStage,
    ) -> crate::tui::receiver::ReceiverTickControl {
        let context = self.receiver_tick_context();
        match self
            .receiver
            .plan_tick_stage(stage, context, std::time::Instant::now())
        {
            crate::tui::receiver::ReceiverDecision::Continue => {
                crate::tui::receiver::ReceiverTickControl::AdvanceStage
            }
            crate::tui::receiver::ReceiverDecision::Stop => {
                crate::tui::receiver::ReceiverTickControl::StopTick
            }
            crate::tui::receiver::ReceiverDecision::Effect(effect) => {
                crate::tui::receiver::control_after_effect(self.execute_receiver_effect(effect))
            }
        }
    }

    fn receiver_tick_context(&self) -> crate::tui::receiver::ReceiverTickContext {
        let queued_actor_matches_session = self
            .receiver
            .next_job()
            .is_some_and(|job| self.brain.session_actor() == Some(&job.actor));
        crate::tui::receiver::ReceiverTickContext {
            brain_turn_active: self.brain.turn_active(),
            panel_open: self.brain.main_controller().is_some(),
            queued_actor_matches_session,
        }
    }

    fn execute_receiver_effect(
        &mut self,
        effect: crate::tui::receiver::ReceiverEffect,
    ) -> crate::tui::receiver::ReceiverEffectOutcome {
        match effect {
            crate::tui::receiver::ReceiverEffect::PollRemoteCompletion(target) => {
                self.poll_completed_remote_response(target);
            }
            crate::tui::receiver::ReceiverEffect::PollInteractiveCompletion { response_id } => {
                self.poll_completed_interactive_turn(&response_id);
            }
            crate::tui::receiver::ReceiverEffect::DeliverProcessingDelay(target) => {
                self.send_processing_delay(target);
            }
            crate::tui::receiver::ReceiverEffect::SamplePanelActivity { sampled_at } => {
                self.sample_panel_activity(sampled_at);
            }
            crate::tui::receiver::ReceiverEffect::LogActivityProbe(probe) => {
                self.log_receiver_activity_probe(&probe);
            }
            crate::tui::receiver::ReceiverEffect::AbandonTimedOutTurn => {
                self.abandon_timed_out_remote_turn();
            }
            crate::tui::receiver::ReceiverEffect::ExpireWarmLease { channel } => {
                crate::logging::log(format!(
                    "receiver session lease expired channel={channel:?}; restoring interactive session"
                ));
                self.close_receiver_panel(true);
            }
            crate::tui::receiver::ReceiverEffect::PollInboundJobs => {
                self.receiver.poll_jobs(self.context.workspace().id());
            }
            crate::tui::receiver::ReceiverEffect::ApplyRestart(plan) => {
                self.apply_receiver_restart(&plan);
            }
            crate::tui::receiver::ReceiverEffect::CheckSyncFreshness => {
                return self.execute_receiver_sync_freshness_effect();
            }
            crate::tui::receiver::ReceiverEffect::ApplyNewSession(job) => {
                self.apply_receiver_new_session(&job);
                return crate::tui::receiver::ReceiverEffectOutcome::NewSessionApplied;
            }
            crate::tui::receiver::ReceiverEffect::CloseIdlePanel { receiver_panel } => {
                self.close_idle_panel_for_receiver_dispatch(receiver_panel);
            }
            crate::tui::receiver::ReceiverEffect::Dispatch(message) => {
                self.dispatch_receiver_message(&message);
            }
        }
        crate::tui::receiver::ReceiverEffectOutcome::Completed
    }

    fn dispatch_receiver_message(&mut self, message: &InboundJob) {
        let label = match message.channel {
            crate::server::receiver::Channel::Sms => "SMS",
            crate::server::receiver::Channel::Email => "email",
        };
        let _delivery_shape = match message.channel {
            crate::server::receiver::Channel::Sms => crate::server::reply::sms(&message.prompt),
            crate::server::receiver::Channel::Email => {
                let _ = crate::server::reply::email_html(&message.prompt);
                crate::server::reply::email(&message.prompt)
            }
        };
        let _ = crate::server::reply::processing_notice(label);
        let staged = crate::server::receiver::stage_attachments(
            self.context.workspace(),
            self.context.command(),
            message,
        );
        let mut attachments = String::new();
        for attachment in staged {
            use std::fmt::Write as _;
            let _ = write!(
                attachments,
                "\nAttachment: {}",
                attachment.path.map_or_else(
                    || format!(
                        "{} (unreadable: {})",
                        attachment.source,
                        attachment
                            .error
                            .unwrap_or_else(|| "unknown error".to_owned())
                    ),
                    |path| path.display().to_string(),
                )
            );
        }
        let prompt = format!(
            "This is an authenticated {label} message from {} (actor {}). Respond as the user's brain. If the message asks to add, create, capture, remember, or track a task, create it in Brain's task system; do not perform the task now unless the sender explicitly asks you to.\n\n{}",
            message.actor.display_name(),
            message.actor.user_id(),
            message.prompt
        );
        // A `/new` on this channel makes the launch that follows it refuse to
        // resume, which is what retires the old conversation.
        self.receiver.prepare_channel_launch(message.channel);
        let reusing_receiver_panel =
            self.receiver.has_receiver_session() && self.brain.main_controller().is_some();
        if reusing_receiver_panel {
            if let Some(session_id) = self.receiver.receiver_response_id() {
                let response_path = self
                    .context
                    .workspace()
                    .paths()
                    .responses_dir()
                    .join(format!("{session_id}.json"));
                let _ = std::fs::remove_file(response_path);
            }
        } else {
            self.receiver.request_receiver_launch(message.actor.clone());
        }
        // A fresh launch passes the prompt as a command argument; warm reuse
        // types it into the live composer. The distinction is essential when
        // diagnosing a message that reached the panel but never submitted.
        crate::logging::log(format!(
            "receiver dispatch delivering channel={:?} via {}",
            message.channel,
            if reusing_receiver_panel {
                "warm-panel injection"
            } else {
                "fresh launch argument"
            }
        ));
        if reusing_receiver_panel {
            // Existing composer contents explain a prompt that lands beside
            // leftover text or behind something waiting on a keypress.
            crate::logging::log(format!(
                "receiver panel before injection: {}",
                self.panel_tail(14)
                    .unwrap_or_else(|| "<no panel>".to_owned())
            ));
        }
        let launched = self.open_or_focus_brain(Some(&(prompt + &attachments)));
        let dispatched_at = std::time::Instant::now();
        let _ = self
            .receiver
            .finish_dispatch(launched, message, dispatched_at);
        if launched {
            crate::logging::log(format!(
                "receiver dispatch started channel={:?} queue_depth={}",
                message.channel,
                self.receiver.pending_count()
            ));
        } else {
            crate::logging::log(format!(
                "receiver dispatch launch failed; message retained channel={:?} queue_depth={}",
                message.channel,
                self.receiver.pending_count()
            ));
        }
    }
}
