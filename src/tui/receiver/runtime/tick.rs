use std::time::{Duration, Instant};

use super::{ReceiverRuntime, RemoteCompletionTarget};
use crate::tui::receiver::decision::{
    ReceiverDecision, ReceiverTickContext, StageDecision, TickFacts, TickStage, decide_stage,
};
use crate::tui::receiver::effect::{ReceiverEffect, ReceiverEffectKind};
use crate::tui::receiver::policy;

impl ReceiverRuntime {
    #[must_use]
    pub(crate) fn plan_tick_stage(
        &mut self,
        stage: TickStage,
        context: ReceiverTickContext,
        now: Instant,
    ) -> ReceiverDecision {
        let facts = self.tick_facts(context, now);
        match decide_stage(stage, facts) {
            StageDecision::Continue => {
                if stage == TickStage::Retry {
                    self.retry_at = None;
                }
                ReceiverDecision::Continue
            }
            StageDecision::Stop => ReceiverDecision::Stop,
            StageDecision::Effect(kind) => self
                .materialize_effect(kind, now)
                .map_or(ReceiverDecision::Continue, ReceiverDecision::Effect),
        }
    }

    fn tick_facts(&self, context: ReceiverTickContext, now: Instant) -> TickFacts {
        let queued_channel = self.next_job().map(|job| job.channel);
        let reusable_channel = context
            .queued_actor_matches_session
            .then(|| self.active_channel())
            .flatten();
        TickFacts {
            remote_turn_active: self.remote_turn_in_flight(),
            remote_completion_tracked: self.active_remote_turn().is_some(),
            brain_turn_active: context.brain_turn_active,
            interactive_completion_tracked: self.interactive_response_id().is_some(),
            processing_delay_due: self.processing_delay_due(now),
            panel_sample_due: self.panel_sample_due(now),
            activity_probe_due: self.activity_probe_due(now),
            timeout_due: self.should_abandon_turn(now),
            warm_lease_expired: self.warm_lease_expired(now).is_some(),
            restart_requested: self.queue.has_restart(),
            retry_waiting: !policy::retry_ready(self.retry_at, now),
            queued_channel,
            new_session_requested: self.queue.has_new_session(),
            panel_open: context.panel_open,
            reusable_channel,
        }
    }

    fn materialize_effect(
        &mut self,
        kind: ReceiverEffectKind,
        now: Instant,
    ) -> Option<ReceiverEffect> {
        match kind {
            ReceiverEffectKind::PollRemoteCompletion => self
                .remote_completion_target()
                .map(ReceiverEffect::PollRemoteCompletion),
            ReceiverEffectKind::PollInteractiveCompletion => self
                .interactive_response_id()
                .map(str::to_owned)
                .map(|response_id| ReceiverEffect::PollInteractiveCompletion { response_id }),
            ReceiverEffectKind::DeliverProcessingDelay => self
                .claim_processing_delay(now)
                .map(ReceiverEffect::DeliverProcessingDelay),
            ReceiverEffectKind::SamplePanelActivity => {
                Some(ReceiverEffect::SamplePanelActivity { sampled_at: now })
            }
            ReceiverEffectKind::LogActivityProbe => self
                .take_due_probe(now)
                .map(ReceiverEffect::LogActivityProbe),
            ReceiverEffectKind::AbandonTimedOutTurn => Some(ReceiverEffect::AbandonTimedOutTurn),
            ReceiverEffectKind::ExpireWarmLease => self
                .warm_lease_expired(now)
                .map(|channel| ReceiverEffect::ExpireWarmLease { channel }),
            ReceiverEffectKind::PollInboundJobs => Some(ReceiverEffect::PollInboundJobs),
            ReceiverEffectKind::ApplyRestart => self
                .take_restart()
                .map(Box::new)
                .map(ReceiverEffect::ApplyRestart),
            ReceiverEffectKind::CheckSyncFreshness => Some(ReceiverEffect::CheckSyncFreshness),
            ReceiverEffectKind::ApplyNewSession => self
                .take_new_session()
                .map(Box::new)
                .map(ReceiverEffect::ApplyNewSession),
            ReceiverEffectKind::CloseIdlePanel => Some(ReceiverEffect::CloseIdlePanel {
                receiver_panel: self.has_receiver_session(),
            }),
            ReceiverEffectKind::Dispatch => self
                .next_job()
                .cloned()
                .map(Box::new)
                .map(ReceiverEffect::Dispatch),
        }
    }

    fn processing_delay_due(&self, now: Instant) -> bool {
        !self.delay_sent
            && self.started.is_some_and(|started| {
                now.saturating_duration_since(started) >= Duration::from_secs(120)
            })
            && self.active_delivery_target().is_some()
    }

    fn activity_probe_due(&self, now: Instant) -> bool {
        self.probe
            .is_some_and(|(due, _)| now >= due && self.started.is_some())
    }

    fn remote_completion_target(&self) -> Option<RemoteCompletionTarget> {
        let turn = self.active_remote_turn()?;
        Some(RemoteCompletionTarget {
            response_id: turn.response_id.to_owned(),
            channel: turn.channel,
            sender: turn.sender.to_owned(),
        })
    }
}
