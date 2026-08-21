use super::decision::{
    ReceiverTickControl, StageDecision, TickFacts, TickStage, control_after_effect, decide_stage,
    run_receiver_tick,
};
use super::effect::{ReceiverEffectKind, ReceiverEffectOutcome};
use crate::server::receiver::Channel;

#[test]
fn production_tick_coordinator_rechecks_only_the_current_stage() {
    let mut visited = Vec::new();
    let mut new_session_controls = 0;

    run_receiver_tick(|stage| {
        visited.push(stage);
        if stage == TickStage::NewSession && new_session_controls < 2 {
            new_session_controls += 1;
            ReceiverTickControl::RepeatCurrentStage
        } else {
            ReceiverTickControl::AdvanceStage
        }
    });

    assert_eq!(
        visited,
        [
            TickStage::RemoteCompletion,
            TickStage::InteractiveCompletion,
            TickStage::ProcessingDelay,
            TickStage::PanelActivity,
            TickStage::ActivityProbe,
            TickStage::TurnTimeout,
            TickStage::WarmLease,
            TickStage::InboundJobs,
            TickStage::Restart,
            TickStage::Retry,
            TickStage::SyncFreshness,
            TickStage::NewSession,
            TickStage::NewSession,
            TickStage::NewSession,
            TickStage::IdlePanel,
            TickStage::Dispatch,
        ]
    );
}

#[test]
fn production_tick_coordinator_stops_before_every_later_stage() {
    let mut visited = Vec::new();

    run_receiver_tick(|stage| {
        visited.push(stage);
        if stage == TickStage::SyncFreshness {
            ReceiverTickControl::StopTick
        } else {
            ReceiverTickControl::AdvanceStage
        }
    });

    assert_eq!(
        visited,
        [
            TickStage::RemoteCompletion,
            TickStage::InteractiveCompletion,
            TickStage::ProcessingDelay,
            TickStage::PanelActivity,
            TickStage::ActivityProbe,
            TickStage::TurnTimeout,
            TickStage::WarmLease,
            TickStage::InboundJobs,
            TickStage::Restart,
            TickStage::Retry,
            TickStage::SyncFreshness,
        ]
    );
}

#[test]
fn semantic_effect_outcomes_select_the_production_tick_control() {
    assert_eq!(
        control_after_effect(ReceiverEffectOutcome::Completed),
        ReceiverTickControl::AdvanceStage
    );
    assert_eq!(
        control_after_effect(ReceiverEffectOutcome::FreshnessPending),
        ReceiverTickControl::StopTick
    );
    assert_eq!(
        control_after_effect(ReceiverEffectOutcome::NewSessionApplied),
        ReceiverTickControl::RepeatCurrentStage
    );
}

#[test]
fn pure_stage_decisions_cover_every_existing_lifecycle_condition() {
    struct Case {
        name: &'static str,
        stage: TickStage,
        facts: TickFacts,
        expected: StageDecision,
    }

    let queued = || TickFacts {
        queued_channel: Some(Channel::Sms),
        ..TickFacts::default()
    };
    let remote = || TickFacts {
        remote_turn_active: true,
        remote_completion_tracked: true,
        ..TickFacts::default()
    };
    let cases = [
        Case {
            name: "disabled",
            // Intent gates server forwarding, not draining a frame already
            // accepted by this TUI's socket.
            stage: TickStage::InboundJobs,
            facts: TickFacts::default(),
            expected: StageDecision::Effect(ReceiverEffectKind::PollInboundJobs),
        },
        Case {
            name: "idle",
            stage: TickStage::InboundJobs,
            facts: TickFacts::default(),
            expected: StageDecision::Effect(ReceiverEffectKind::PollInboundJobs),
        },
        Case {
            name: "queued",
            stage: TickStage::Dispatch,
            facts: TickFacts {
                brain_turn_active: true,
                queued_channel: Some(Channel::Sms),
                ..TickFacts::default()
            },
            expected: StageDecision::Continue,
        },
        Case {
            name: "waiting on sync freshness",
            stage: TickStage::SyncFreshness,
            facts: queued(),
            expected: StageDecision::Effect(ReceiverEffectKind::CheckSyncFreshness),
        },
        Case {
            name: "eligible to dispatch",
            stage: TickStage::Dispatch,
            facts: queued(),
            expected: StageDecision::Effect(ReceiverEffectKind::Dispatch),
        },
        Case {
            name: "matching warm channel",
            stage: TickStage::Dispatch,
            facts: TickFacts {
                panel_open: true,
                reusable_channel: Some(Channel::Sms),
                ..queued()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::Dispatch),
        },
        Case {
            name: "different warm channel",
            stage: TickStage::IdlePanel,
            facts: TickFacts {
                panel_open: true,
                reusable_channel: Some(Channel::Email),
                ..queued()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::CloseIdlePanel),
        },
        Case {
            name: "interactive turn busy",
            stage: TickStage::InteractiveCompletion,
            facts: TickFacts {
                brain_turn_active: true,
                interactive_completion_tracked: true,
                queued_channel: Some(Channel::Sms),
                ..TickFacts::default()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::PollInteractiveCompletion),
        },
        Case {
            name: "active remote turn",
            stage: TickStage::Dispatch,
            facts: TickFacts {
                remote_turn_active: true,
                queued_channel: Some(Channel::Sms),
                ..TickFacts::default()
            },
            expected: StageDecision::Continue,
        },
        Case {
            name: "panel activity sample due",
            stage: TickStage::PanelActivity,
            facts: TickFacts {
                panel_sample_due: true,
                ..remote()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::SamplePanelActivity),
        },
        Case {
            name: "activity probe due",
            stage: TickStage::ActivityProbe,
            facts: TickFacts {
                activity_probe_due: true,
                ..remote()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::LogActivityProbe),
        },
        Case {
            name: "delay notice due",
            stage: TickStage::ProcessingDelay,
            facts: TickFacts {
                processing_delay_due: true,
                ..remote()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::DeliverProcessingDelay),
        },
        Case {
            name: "turn timeout due",
            stage: TickStage::TurnTimeout,
            facts: TickFacts {
                timeout_due: true,
                ..remote()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::AbandonTimedOutTurn),
        },
        Case {
            name: "completion available",
            stage: TickStage::RemoteCompletion,
            facts: remote(),
            expected: StageDecision::Effect(ReceiverEffectKind::PollRemoteCompletion),
        },
        Case {
            name: "lease expiry",
            stage: TickStage::WarmLease,
            facts: TickFacts {
                warm_lease_expired: true,
                ..TickFacts::default()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::ExpireWarmLease),
        },
        Case {
            name: "retry waiting",
            stage: TickStage::Retry,
            facts: TickFacts {
                retry_waiting: true,
                ..queued()
            },
            expected: StageDecision::Stop,
        },
        Case {
            name: "restart requested",
            stage: TickStage::Restart,
            facts: TickFacts {
                restart_requested: true,
                ..TickFacts::default()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::ApplyRestart),
        },
        Case {
            name: "new-session requested",
            stage: TickStage::NewSession,
            facts: TickFacts {
                new_session_requested: true,
                queued_channel: Some(Channel::Sms),
                ..TickFacts::default()
            },
            expected: StageDecision::Effect(ReceiverEffectKind::ApplyNewSession),
        },
    ];

    for case in cases {
        assert_eq!(
            decide_stage(case.stage, case.facts),
            case.expected,
            "{} lifecycle decision changed",
            case.name
        );
    }
}

#[test]
fn warm_panel_reuse_and_replacement_are_symmetric_for_sms_and_email() {
    for (queued, panel) in [
        (Channel::Sms, Channel::Sms),
        (Channel::Email, Channel::Email),
    ] {
        let facts = TickFacts {
            queued_channel: Some(queued),
            panel_open: true,
            reusable_channel: Some(panel),
            ..TickFacts::default()
        };
        assert_eq!(
            decide_stage(TickStage::Dispatch, facts),
            StageDecision::Effect(ReceiverEffectKind::Dispatch),
            "{queued:?} must reuse its own warm panel"
        );
    }

    for (queued, panel) in [
        (Channel::Sms, Channel::Email),
        (Channel::Email, Channel::Sms),
    ] {
        let facts = TickFacts {
            queued_channel: Some(queued),
            panel_open: true,
            reusable_channel: Some(panel),
            ..TickFacts::default()
        };
        assert_eq!(
            decide_stage(TickStage::IdlePanel, facts),
            StageDecision::Effect(ReceiverEffectKind::CloseIdlePanel),
            "{queued:?} must replace a {panel:?} warm panel"
        );
    }
}
