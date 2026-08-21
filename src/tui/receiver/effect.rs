//! Cross-boundary work requested by receiver-local decisions.

use crate::server::receiver::{Channel, InboundJob, RestartPlan};

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

pub(crate) enum ReceiverEffect {
    PollRemoteCompletion(RemoteCompletionTarget),
    PollInteractiveCompletion { response_id: String },
    DeliverProcessingDelay(DeliveryTarget),
    SamplePanelActivity { sampled_at: std::time::Instant },
    LogActivityProbe(ReceiverProbe),
    AbandonTimedOutTurn,
    ExpireWarmLease { channel: Channel },
    PollInboundJobs,
    ApplyRestart(Box<RestartPlan<InboundJob>>),
    CheckSyncFreshness,
    ApplyNewSession(Box<InboundJob>),
    CloseIdlePanel { receiver_panel: bool },
    Dispatch(Box<InboundJob>),
}
