use crate::tui::App;

/// Whether the deferred triage gate should resolve now.
///
/// Resolves only when a strictly-newer sync-journal row exists than the one
/// seen at arm time. A timeout must not resolve this gate because that would
/// evaluate stale local habits while the startup sync is still running or has
/// failed. Pure so the decision is unit-tested without a clock or a DB.
pub(super) fn triage_gate_resolved(seen: Option<i64>, latest: Option<i64>) -> bool {
    match (latest, seen) {
        (Some(l), Some(s)) => l > s,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tui::app_actions) enum TriageAlertEvent {
    PaletteEnabled,
    RefreshSucceeded,
}

/// Decide whether an alert check runs now or remains deferred to fresh state.
pub(in crate::tui::app_actions) fn should_check_daily_triage(
    event: TriageAlertEvent,
    refresh_gate_active: bool,
    alert_disabled: bool,
) -> bool {
    !alert_disabled
        && match event {
            TriageAlertEvent::PaletteEnabled => !refresh_gate_active,
            TriageAlertEvent::RefreshSucceeded => true,
        }
}

impl App {
    /// Write the live daily-triage-alert state to portable config.
    ///
    /// The palette row is the same decision as `brain config set
    /// enable_daily_triage_check=…`, so it is stored the same way: a user who
    /// silences the nudge means it, and would not expect it back at the next
    /// launch or on their other machine. A write failure never fails the toggle
    /// — the running session still honors it — but it is surfaced, because
    /// silently degrading a persistent choice to a session one is the surprise
    /// this exists to avoid.
    pub(crate) fn persist_daily_triage_check(&self) -> anyhow::Result<()> {
        crate::settings::set(
            &self.command_context.workspace,
            "enable_daily_triage_check",
            if self.skip_daily_triage_check {
                "false"
            } else {
                "true"
            },
        )
    }
}

/// What the post-sync refresh should do about the daily-triage nudge. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriageAlertResolution {
    /// Triage is still outstanding and no nudge is up: raise it.
    Open,
    /// The sync proved triage is already done (another machine, or a peer) while
    /// the nudge was on screen: take it away.
    Dismiss,
    /// Triage is outstanding, but another captive overlay owns the one modal
    /// slot. Keep the startup decision pending until that overlay closes.
    Defer,
    /// Nothing to change.
    Leave,
}

/// How the shell's exclusive overlay slot relates to the triage nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriageAlertOccupancy {
    Empty,
    TriageNudge,
    OtherOverlay,
}

/// Reconcile the on-screen nudge with what the completed sync actually showed.
///
/// The nudge is raised immediately at startup rather than waiting for the sync,
/// so the shell is usable at once — which means it can be showing a question the
/// sync is about to answer. When the refreshed habits say triage was already
/// completed today, an open nudge is stale and is withdrawn rather than left for
/// the user to dismiss (and possibly answer, re-running a pass that already ran).
#[must_use]
pub(crate) const fn resolve_triage_alert(
    triage_outstanding: bool,
    occupancy: TriageAlertOccupancy,
) -> TriageAlertResolution {
    match (triage_outstanding, occupancy) {
        (true, TriageAlertOccupancy::Empty) => TriageAlertResolution::Open,
        (true, TriageAlertOccupancy::OtherOverlay) => TriageAlertResolution::Defer,
        (false, TriageAlertOccupancy::TriageNudge) => TriageAlertResolution::Dismiss,
        _ => TriageAlertResolution::Leave,
    }
}

/// Whether the startup gate must remain alive for a later overlay-free tick.
#[must_use]
pub(crate) const fn triage_reconciliation_pending(resolution: TriageAlertResolution) -> bool {
    matches!(resolution, TriageAlertResolution::Defer)
}

/// The "logical day" a local instant belongs to for triage purposes. The
/// day boundary is shifted from midnight to `rollover_hour` (local): the
/// hours between midnight and the rollover still count as the previous day,
/// so a session running past midnight isn't treated as a new day until the
/// rollover hour. An out-of-range hour (>23) falls back to the 6 AM default.
pub(super) fn logical_day(now: chrono::NaiveDateTime, rollover_hour: u32) -> chrono::NaiveDate {
    let hour = if rollover_hour <= 23 {
        rollover_hour
    } else {
        6
    };
    (now - chrono::Duration::hours(i64::from(hour))).date()
}

/// Decide whether `now` has crossed into a new logical day since the triage
/// check last ran on `last_checked`. Returns `Some(new_day)` when the day
/// changed (the caller should re-run the triage nudge and adopt `new_day` as
/// "today"), or `None` when we're still within the same logical day — the
/// midnight-to-`rollover_hour` window included, so nothing fires there.
pub(super) fn triage_rollover(
    last_checked: chrono::NaiveDate,
    now: chrono::NaiveDateTime,
    rollover_hour: u32,
) -> Option<chrono::NaiveDate> {
    let day = logical_day(now, rollover_hour);
    (day != last_checked).then_some(day)
}
