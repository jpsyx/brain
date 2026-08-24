//! App-local ownership of one durable receiver run between event-loop ticks.

use crate::state::{ReceiverRunClaim, ReceiverSessionAttribution};
use crate::tui::model::SessionTabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverEffectOutcome {
    Completed,
    FreshnessPending,
}

pub(crate) enum DurableReceiverRun {
    Idle,
    Claimed(ClaimedReceiverRun),
    Active(ActiveReceiverRun),
}

pub(crate) struct ClaimedReceiverRun {
    pub(crate) claim: ReceiverRunClaim,
    pub(crate) remote: super::ReceiverRemoteSession,
}

pub(crate) struct ActiveReceiverRun {
    pub(crate) claim: ReceiverRunClaim,
    pub(crate) attribution: ReceiverSessionAttribution,
    pub(crate) tab_id: SessionTabId,
}
