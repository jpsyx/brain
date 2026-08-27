//! App-local ownership of one durable receiver run between event-loop ticks.

use crate::state::{ReceiverReconciliationEffect, ReceiverRunClaim, ReceiverSessionAttribution};
use crate::tui::model::SessionTabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverEffectOutcome {
    Completed,
    FreshnessPending,
}

pub(crate) enum DurableReceiverRun {
    Idle,
    Claimed(ClaimedReceiverRun),
    RecoveryClaimed(ClaimedReceiverRun),
    Active(ActiveReceiverRun),
    CleanupPending(CleanupPendingReceiverRun),
}

pub(crate) struct ClaimedReceiverRun {
    pub(crate) claim: ReceiverRunClaim,
    pub(crate) remote: super::ReceiverRemoteSession,
    pub(crate) freshness_ready: bool,
}

pub(crate) struct ActiveReceiverRun {
    pub(crate) claim: ReceiverRunClaim,
    pub(crate) attribution: ReceiverSessionAttribution,
    pub(crate) tab_id: SessionTabId,
    pub(crate) _attachments: super::attachments::PreparedReceiverAttachments,
}

pub(crate) struct CleanupPendingReceiverRun {
    pub(crate) active: ActiveReceiverRun,
    pub(crate) effect: ReceiverReconciliationEffect,
    pub(crate) shutdown_complete: bool,
    pub(crate) artifacts_removed: bool,
    pub(crate) defer_once: bool,
}
