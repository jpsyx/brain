use regex::RegexBuilder;

use crate::tasks::task::Task;
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
    /// Nothing to change.
    Leave,
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
    nudge_is_open: bool,
) -> TriageAlertResolution {
    match (triage_outstanding, nudge_is_open) {
        (true, false) => TriageAlertResolution::Open,
        (false, true) => TriageAlertResolution::Dismiss,
        _ => TriageAlertResolution::Leave,
    }
}

/// Gate the daily-triage nudge on the process-scoped opt-out (seeded from
/// `enable_daily_triage_check`, flipped by the palette) before consulting
/// [`triage_nudge_target`]. When `disabled` is set the
/// modal never fires this run regardless of habit state; this is a per-process
/// flag, not a persistent config change. Pure so the opt-out is unit-tested
/// without constructing an `App`.
pub(super) fn triage_modal_target<'h>(
    enable_triage_habits: bool,
    disabled: bool,
    habits: &'h [Task],
    pattern: &str,
    today: chrono::NaiveDate,
) -> Option<&'h Task> {
    if !enable_triage_habits || disabled {
        return None;
    }
    triage_nudge_target(habits, pattern, today)
}

/// Decide whether the startup triage nudge should fire, and for which habit.
///
/// Returns `Some(habit)` — the occurrence to surface in the modal — when the
/// `pattern` matches at least one habit by name but *no* matched occurrence
/// is completed today. Returns `None` (no nudge) when the pattern is
/// empty/blank, is an invalid regex, matches no habit at all, or some matched
/// occurrence is already completed today.
///
/// Matching by a case-insensitive name regex (rather than a fixed ID) is what
/// makes this correct across recurrence: the triage habit gets a fresh ID
/// each cycle, but its name is stable, so "is today's triage done?" reduces to
/// "does any occurrence of the named habit carry today's `completed_date`?".
///
/// When a nudge is warranted, the surfaced occurrence is the one due today if
/// present, otherwise the latest-dated match — the row the user is most likely
/// to recognize as "the current one".
pub(super) fn triage_nudge_target<'h>(
    habits: &'h [Task],
    pattern: &str,
    today: chrono::NaiveDate,
) -> Option<&'h Task> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return None;
    }
    let re = RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()?;

    let matched: Vec<&Task> = habits.iter().filter(|h| re.is_match(&h.name)).collect();
    if matched.is_empty() {
        // Habit not installed (or pattern doesn't match anything) — the
        // nudge is optional, so stay silent rather than guess.
        return None;
    }
    if matched.iter().any(|h| h.is_completed_today(today)) {
        // Triage already done for today's cycle.
        return None;
    }

    matched
        .iter()
        .find(|h| h.due_date == Some(today))
        .or_else(|| matched.iter().max_by_key(|h| h.due_date))
        .copied()
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
