//! Cross-boundary work requested by receiver-local decisions.

use crate::server::receiver::Channel;

use super::runtime::{DeliveryTarget, ReceiverProbe, RemoteCompletionTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverEffectKind {
    PollRemoteCompletion,
    PollInteractiveCompletion,
    DeliverProcessingDelay,
    SamplePanelActivity,
    LogActivityProbe,
    AbandonTimedOutTurn,
    ExpireWarmLease,
    PollInboundJobs,
    ApplyRestart,
    CheckSyncFreshness,
    ApplyNewSession,
    CloseIdlePanel,
    Dispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverEffectOutcome {
    Completed,
    FreshnessPending,
    NewSessionApplied,
}

pub(crate) enum ReceiverEffect {
    PollRemoteCompletion(RemoteCompletionTarget),
    PollInteractiveCompletion {
        response_id: String,
    },
    DeliverProcessingDelay(DeliveryTarget),
    SamplePanelActivity {
        sampled_at: std::time::Instant,
    },
    LogActivityProbe(ReceiverProbe),
    AbandonTimedOutTurn,
    ExpireWarmLease {
        channel: Channel,
    },
    PollInboundJobs,
    ApplyRestart(
        std::boxed::Box<crate::server::receiver::RestartPlan<crate::server::receiver::InboundJob>>,
    ),
    CheckSyncFreshness,
    ApplyNewSession(std::boxed::Box<crate::server::receiver::InboundJob>),
    CloseIdlePanel {
        receiver_panel: bool,
    },
    Dispatch(std::boxed::Box<crate::server::receiver::InboundJob>),
}
