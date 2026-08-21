use super::decision::{TickFacts, plan_tick};
use super::effect::ReceiverEffectKind;
use crate::server::receiver::Channel;

#[test]
fn ordered_tick_plan_covers_every_existing_lifecycle_condition() {
    struct Case {
        name: &'static str,
        facts: TickFacts,
        expected: &'static [ReceiverEffectKind],
    }

    let queued = || TickFacts {
        queued_channel: Some(Channel::Sms),
        sync_ready: true,
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
            facts: TickFacts::default(),
            expected: &[ReceiverEffectKind::PollInboundJobs],
        },
        Case {
            name: "idle",
            facts: TickFacts::default(),
            expected: &[ReceiverEffectKind::PollInboundJobs],
        },
        Case {
            name: "queued",
            facts: TickFacts {
                brain_turn_active: true,
                queued_channel: Some(Channel::Sms),
                sync_ready: true,
                ..TickFacts::default()
            },
            expected: &[ReceiverEffectKind::PollInboundJobs],
        },
        Case {
            name: "waiting on sync freshness",
            facts: TickFacts {
                sync_ready: false,
                ..queued()
            },
            expected: &[
                ReceiverEffectKind::PollInboundJobs,
                ReceiverEffectKind::CheckSyncFreshness,
            ],
        },
        Case {
            name: "eligible to dispatch",
            facts: queued(),
            expected: &[
                ReceiverEffectKind::PollInboundJobs,
                ReceiverEffectKind::CheckSyncFreshness,
                ReceiverEffectKind::Dispatch,
            ],
        },
        Case {
            name: "matching warm channel",
            facts: TickFacts {
                panel_open: true,
                reusable_channel: Some(Channel::Sms),
                ..queued()
            },
            expected: &[
                ReceiverEffectKind::PollInboundJobs,
                ReceiverEffectKind::CheckSyncFreshness,
                ReceiverEffectKind::Dispatch,
            ],
        },
        Case {
            name: "different warm channel",
            facts: TickFacts {
                panel_open: true,
                reusable_channel: Some(Channel::Email),
                ..queued()
            },
            expected: &[
                ReceiverEffectKind::PollInboundJobs,
                ReceiverEffectKind::CheckSyncFreshness,
                ReceiverEffectKind::CloseIdlePanel,
                ReceiverEffectKind::Dispatch,
            ],
        },
        Case {
            name: "interactive turn busy",
            facts: TickFacts {
                brain_turn_active: true,
                interactive_completion_tracked: true,
                queued_channel: Some(Channel::Sms),
                sync_ready: true,
                ..TickFacts::default()
            },
            expected: &[
                ReceiverEffectKind::PollInteractiveCompletion,
                ReceiverEffectKind::PollInboundJobs,
            ],
        },
        Case {
            name: "active remote turn",
            facts: TickFacts {
                remote_turn_active: true,
                ..TickFacts::default()
            },
            expected: &[ReceiverEffectKind::PollInboundJobs],
        },
        Case {
            name: "activity probe due",
            facts: TickFacts {
                panel_sample_due: true,
                activity_probe_due: true,
                ..remote()
            },
            expected: &[
                ReceiverEffectKind::PollRemoteCompletion,
                ReceiverEffectKind::SamplePanelActivity,
                ReceiverEffectKind::LogActivityProbe,
                ReceiverEffectKind::PollInboundJobs,
            ],
        },
        Case {
            name: "timeout and delay notice due",
            facts: TickFacts {
                processing_delay_due: true,
                timeout_due: true,
                ..remote()
            },
            expected: &[
                ReceiverEffectKind::PollRemoteCompletion,
                ReceiverEffectKind::DeliverProcessingDelay,
                ReceiverEffectKind::AbandonTimedOutTurn,
                ReceiverEffectKind::PollInboundJobs,
            ],
        },
        Case {
            name: "completion available",
            facts: remote(),
            expected: &[
                ReceiverEffectKind::PollRemoteCompletion,
                ReceiverEffectKind::PollInboundJobs,
            ],
        },
        Case {
            name: "lease expiry",
            facts: TickFacts {
                warm_lease_expired: true,
                ..TickFacts::default()
            },
            expected: &[
                ReceiverEffectKind::ExpireWarmLease,
                ReceiverEffectKind::PollInboundJobs,
            ],
        },
        Case {
            name: "retry waiting",
            facts: TickFacts {
                retry_waiting: true,
                ..queued()
            },
            expected: &[ReceiverEffectKind::PollInboundJobs],
        },
        Case {
            name: "restart requested",
            facts: TickFacts {
                restart_requested: true,
                ..TickFacts::default()
            },
            expected: &[
                ReceiverEffectKind::PollInboundJobs,
                ReceiverEffectKind::ApplyRestart,
            ],
        },
        Case {
            name: "new-session requested",
            facts: TickFacts {
                new_session_requested: true,
                queued_channel: Some(Channel::Sms),
                sync_ready: true,
                ..TickFacts::default()
            },
            expected: &[
                ReceiverEffectKind::PollInboundJobs,
                ReceiverEffectKind::CheckSyncFreshness,
                ReceiverEffectKind::ApplyNewSession,
            ],
        },
    ];

    for case in cases {
        assert_eq!(
            plan_tick(case.facts).effects(),
            case.expected,
            "{} lifecycle plan changed",
            case.name
        );
    }
}

#[test]
fn tick_plan_preserves_effect_order_when_every_independent_stage_is_due() {
    let facts = TickFacts {
        remote_turn_active: true,
        remote_completion_tracked: true,
        processing_delay_due: true,
        panel_sample_due: true,
        activity_probe_due: true,
        timeout_due: true,
        restart_requested: true,
        queued_channel: Some(Channel::Email),
        sync_ready: true,
        ..TickFacts::default()
    };

    assert_eq!(
        plan_tick(facts).effects(),
        [
            ReceiverEffectKind::PollRemoteCompletion,
            ReceiverEffectKind::DeliverProcessingDelay,
            ReceiverEffectKind::SamplePanelActivity,
            ReceiverEffectKind::LogActivityProbe,
            ReceiverEffectKind::AbandonTimedOutTurn,
            ReceiverEffectKind::PollInboundJobs,
            ReceiverEffectKind::ApplyRestart,
        ]
    );
}
