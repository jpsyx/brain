use std::time::Instant;

use crate::agent::AgentController;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Tasks,
    Brain,
}

/// Which session is showing inside the brain panel. The panel normally hosts a
/// single persistent session ([`BrainTab::Main`]); a [`BrainTab::Session`] is one
/// of the ephemeral *skill sessions* that appear as extra tabs only while they
/// run (see `App::skill_sessions` and [`crate::skill_session`]). Selected with
/// `Alt+1` for the main session and `Alt+<n>` for the nth skill session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrainTab {
    Main,
    Session(SessionTabId),
}

/// Identity of one open skill-session tab, unique for the life of the shell.
///
/// Tabs are addressed by this rather than by list position or by
/// [`crate::skill_session::SkillSessionKey`] so that closing one tab can never
/// silently repoint the active tab at another, and so a definition edited in
/// `skill_sessions` mid-session still resolves to the tab it opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionTabId(pub(crate) u32);

/// One open skill-session tab: which session it runs, what its tab is called,
/// the one-time completion token brain matches, and its ephemeral controller.
pub(crate) struct SkillSessionTab {
    pub(crate) id: SessionTabId,
    pub(crate) key: crate::skill_session::SkillSessionKey,
    pub(crate) title: String,
    pub(crate) token: String,
    pub(crate) controller: AgentController,
}

/// Deferral state for the startup daily-triage nudge while a background sync is
/// running or its refreshed decision is waiting for the overlay slot. See
/// `App::triage_gate` and `App::tick_triage_gate`.
pub(crate) struct TriageGate {
    /// Newest sync-journal row id when the gate was armed; the gate resolves
    /// once a strictly-newer row appears (a background sync finished). `None`
    /// when the journal was empty at arm time.
    pub(crate) seen_journal_id: Option<i64>,
    /// Next instant we're allowed to poll the journal, to throttle the DB reads
    /// down from the 50ms event-loop tick.
    pub(crate) next_poll: Instant,
    /// The sync result and refreshed task state have already been applied. A
    /// true value means only delivery of an outstanding nudge remains.
    pub(crate) refresh_complete: bool,
}

pub(crate) struct ReceiverSyncGate {
    pub(crate) seen_journal_id: Option<i64>,
    pub(crate) launched_at: Instant,
    pub(crate) next_poll: Instant,
    pub(crate) attempts: u8,
}
