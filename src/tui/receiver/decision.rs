//! Pure ordering and readiness decisions for one receiver tick.

use crate::server::receiver::Channel;

use super::effect::{ReceiverEffect, ReceiverEffectKind, ReceiverEffectOutcome};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReceiverTickContext {
    pub(crate) brain_turn_active: bool,
    pub(crate) panel_open: bool,
    pub(crate) queued_actor_matches_session: bool,
}

pub(crate) enum ReceiverDecision {
    Continue,
    Stop,
    Effect(ReceiverEffect),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TickStage {
    RemoteCompletion,
    InteractiveCompletion,
    ProcessingDelay,
    PanelActivity,
    ActivityProbe,
    TurnTimeout,
    WarmLease,
    InboundJobs,
    Restart,
    Retry,
    SyncFreshness,
    NewSession,
    IdlePanel,
    Dispatch,
}

impl TickStage {
    pub(crate) const ORDERED: [Self; 14] = [
        Self::RemoteCompletion,
        Self::InteractiveCompletion,
        Self::ProcessingDelay,
        Self::PanelActivity,
        Self::ActivityProbe,
        Self::TurnTimeout,
        Self::WarmLease,
        Self::InboundJobs,
        Self::Restart,
        Self::Retry,
        Self::SyncFreshness,
        Self::NewSession,
        Self::IdlePanel,
        Self::Dispatch,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverTickControl {
    AdvanceStage,
    StopTick,
    RepeatCurrentStage,
}

#[must_use]
pub(crate) const fn control_after_effect(outcome: ReceiverEffectOutcome) -> ReceiverTickControl {
    match outcome {
        ReceiverEffectOutcome::Completed => ReceiverTickControl::AdvanceStage,
        ReceiverEffectOutcome::FreshnessPending => ReceiverTickControl::StopTick,
        ReceiverEffectOutcome::NewSessionApplied => ReceiverTickControl::RepeatCurrentStage,
    }
}

pub(crate) fn run_receiver_tick(mut execute_stage: impl FnMut(TickStage) -> ReceiverTickControl) {
    for stage in TickStage::ORDERED {
        loop {
            match execute_stage(stage) {
                ReceiverTickControl::AdvanceStage => break,
                ReceiverTickControl::StopTick => return,
                ReceiverTickControl::RepeatCurrentStage => {}
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StageDecision {
    Continue,
    Stop,
    Effect(ReceiverEffectKind),
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TickFacts {
    pub(crate) remote_turn_active: bool,
    pub(crate) remote_completion_tracked: bool,
    pub(crate) brain_turn_active: bool,
    pub(crate) interactive_completion_tracked: bool,
    pub(crate) processing_delay_due: bool,
    pub(crate) panel_sample_due: bool,
    pub(crate) activity_probe_due: bool,
    pub(crate) timeout_due: bool,
    pub(crate) warm_lease_expired: bool,
    pub(crate) restart_requested: bool,
    pub(crate) retry_waiting: bool,
    pub(crate) queued_channel: Option<Channel>,
    pub(crate) new_session_requested: bool,
    pub(crate) panel_open: bool,
    pub(crate) reusable_channel: Option<Channel>,
}

#[must_use]
pub(crate) fn decide_stage(stage: TickStage, facts: TickFacts) -> StageDecision {
    let panel_free = !facts.brain_turn_active && !facts.remote_turn_active;
    match stage {
        TickStage::RemoteCompletion => {
            if facts.remote_turn_active && facts.remote_completion_tracked {
                StageDecision::Effect(ReceiverEffectKind::PollRemoteCompletion)
            } else {
                StageDecision::Continue
            }
        }
        TickStage::InteractiveCompletion => {
            if facts.brain_turn_active
                && !facts.remote_turn_active
                && facts.interactive_completion_tracked
            {
                StageDecision::Effect(ReceiverEffectKind::PollInteractiveCompletion)
            } else {
                StageDecision::Continue
            }
        }
        TickStage::ProcessingDelay => effect_if(
            facts.processing_delay_due,
            ReceiverEffectKind::DeliverProcessingDelay,
        ),
        TickStage::PanelActivity => effect_if(
            facts.panel_sample_due,
            ReceiverEffectKind::SamplePanelActivity,
        ),
        TickStage::ActivityProbe => effect_if(
            facts.activity_probe_due,
            ReceiverEffectKind::LogActivityProbe,
        ),
        TickStage::TurnTimeout => {
            effect_if(facts.timeout_due, ReceiverEffectKind::AbandonTimedOutTurn)
        }
        TickStage::WarmLease => effect_if(
            facts.warm_lease_expired,
            ReceiverEffectKind::ExpireWarmLease,
        ),
        TickStage::InboundJobs => StageDecision::Effect(ReceiverEffectKind::PollInboundJobs),
        TickStage::Restart => effect_if(facts.restart_requested, ReceiverEffectKind::ApplyRestart),
        TickStage::Retry => {
            if facts.retry_waiting {
                StageDecision::Stop
            } else {
                StageDecision::Continue
            }
        }
        TickStage::SyncFreshness => {
            if facts.queued_channel.is_some() && panel_free {
                StageDecision::Effect(ReceiverEffectKind::CheckSyncFreshness)
            } else {
                StageDecision::Continue
            }
        }
        TickStage::NewSession => effect_if(
            panel_free && facts.new_session_requested,
            ReceiverEffectKind::ApplyNewSession,
        ),
        TickStage::IdlePanel => {
            if facts.queued_channel.is_some()
                && panel_free
                && facts.panel_open
                && facts.queued_channel != facts.reusable_channel
            {
                StageDecision::Effect(ReceiverEffectKind::CloseIdlePanel)
            } else {
                StageDecision::Continue
            }
        }
        TickStage::Dispatch => {
            if facts.queued_channel.is_some()
                && panel_free
                && (!facts.panel_open || facts.queued_channel == facts.reusable_channel)
            {
                StageDecision::Effect(ReceiverEffectKind::Dispatch)
            } else {
                StageDecision::Continue
            }
        }
    }
}

const fn effect_if(condition: bool, effect: ReceiverEffectKind) -> StageDecision {
    if condition {
        StageDecision::Effect(effect)
    } else {
        StageDecision::Continue
    }
}
