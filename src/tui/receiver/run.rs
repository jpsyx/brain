//! App-local ownership of one durable receiver run between event-loop ticks.

use crate::agent::AgentController;
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
    RecoveryPreSpawnCleanup(PreSpawnRecoveryCleanup),
    RecoverySpawned(SpawnedRecoveryRun),
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

pub(crate) enum PreSpawnRecoveryOutcome {
    RestoreClaim,
    Lost,
    Failure(crate::state::ReceiverRecoveryFailure),
    ResumeUnavailable,
}

pub(crate) struct PreSpawnRecoveryCleanup {
    pub(crate) claimed: ClaimedReceiverRun,
    pub(crate) controller: AgentController,
    pub(crate) attribution: Option<ReceiverSessionAttribution>,
    pub(crate) outcome: PreSpawnRecoveryOutcome,
    pub(crate) cleanup_authority: ReceiverCleanupAuthority,
    pub(crate) shutdown_complete: bool,
    pub(crate) defer_once: bool,
}

pub(crate) enum ReceiverCleanupAuthority {
    Unresolved,
    Exact(ReceiverReconciliationEffect),
}

pub(crate) enum SpawnedRecoveryStage {
    PostSpawnOwner(AgentController),
    PostAllocationOwner(SessionTabId),
    CleanupDetached(AgentController),
    CleanupTabbed(SessionTabId),
}

pub(crate) struct SpawnedRecoveryRun {
    pub(crate) claimed: ClaimedReceiverRun,
    pub(crate) attribution: ReceiverSessionAttribution,
    pub(crate) pid: i32,
    pub(crate) stage: SpawnedRecoveryStage,
    pub(crate) durable_launch_committed: bool,
    pub(crate) cleanup_authority: ReceiverCleanupAuthority,
    pub(crate) shutdown_complete: bool,
    pub(crate) artifacts_removed: bool,
    pub(crate) defer_once: bool,
}

pub(crate) struct CleanupPendingReceiverRun {
    pub(crate) active: ActiveReceiverRun,
    pub(crate) effect: ReceiverReconciliationEffect,
    pub(crate) shutdown_complete: bool,
    pub(crate) artifacts_removed: bool,
    pub(crate) defer_once: bool,
}
