//! The daily-triage nudge: the Yes/Skip handoffs to the brain, the startup +
//! per-refresh check for an uncompleted triage habit, and the pure
//! name-match / logical-day / rollover helpers (with their unit tests).

use regex::RegexBuilder;

use crate::tasks::task::Task;
use crate::tui::*;

impl App<'_> {
    /// Yes-path for the startup daily-triage modal. Always sends
    /// `/triage` so the user gets the documented triage flow regardless
    /// of which habit ID was configured to gate the prompt.
    pub(crate) fn run_triage(&mut self) {
        self.send_brain_prompt("/triage");
    }

    /// Skip-path for the startup daily-triage modal. Hands off to the brain
    /// with [`SKIP_TRIAGE_PROMPT`] — the `/triage` + `/todo` skills'
    /// documented "skip daily triage" trigger — so the brain marks today's
    /// Morning Triage habit done and runs no triage pass.
    pub(crate) fn skip_triage(&mut self) {
        self.send_brain_prompt(SKIP_TRIAGE_PROMPT);
    }

    /// Match the configured daily-triage habit by name; if no occurrence of
    /// it is completed today, open the triage-confirm modal. No-op when the
    /// config disables the check (empty `daily_triage_name_pattern`), the
    /// pattern is an invalid regex, or no habit matches it — a silent skip
    /// is the right failure mode here because the modal is a nudge, not a
    /// blocker. See [`triage_nudge_target`] for the matching logic.
    pub(crate) fn check_daily_triage(&mut self) {
        if let Some(habit) = triage_nudge_target(
            &self.all_habits,
            &self.config.daily_triage_name_pattern,
            self.today,
        ) {
            self.confirm = Some(ConfirmState::run_triage(
                habit.id.clone(),
                habit.name.clone(),
            ));
        }
    }

    /// Record the logical day the startup triage check ran for. Called once
    /// by `run_tui` after the startup check so [`Self::advance_triage_day`]
    /// only re-fires the nudge on a genuine day rollover, not on the first
    /// refresh of the same day.
    pub(crate) fn seed_triage_day(&mut self, now: chrono::NaiveDateTime) {
        self.triage_day = logical_day(now, self.config.day_rollover_hour);
    }

    /// If `now` has crossed into a new logical day since the triage check last
    /// ran, adopt that day as `today` (which also refreshes every
    /// date-relative view) and return `true` so the caller re-runs the nudge.
    /// A tasks session can stay open for days, so each refresh (`r`) re-checks
    /// whether a new day has begun. The day boundary is
    /// `config.day_rollover_hour` (not midnight), so working past midnight
    /// isn't treated as a new day until that hour. Returns `false` — no
    /// rollover — within the same logical day.
    ///
    /// The caller runs `check_daily_triage` *after* reloading habits so the
    /// nudge sees the freshest completion state (triage may have been marked
    /// done elsewhere during the long-open session).
    pub(crate) fn advance_triage_day(&mut self, now: chrono::NaiveDateTime) -> bool {
        match triage_rollover(self.triage_day, now, self.config.day_rollover_hour) {
            Some(day) => {
                self.today = day;
                self.triage_day = day;
                true
            }
            None => false,
        }
    }
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
fn triage_nudge_target<'h>(
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
fn logical_day(now: chrono::NaiveDateTime, rollover_hour: u32) -> chrono::NaiveDate {
    let hour = if rollover_hour <= 23 { rollover_hour } else { 6 };
    (now - chrono::Duration::hours(i64::from(hour))).date()
}

/// Decide whether `now` has crossed into a new logical day since the triage
/// check last ran on `last_checked`. Returns `Some(new_day)` when the day
/// changed (the caller should re-run the triage nudge and adopt `new_day` as
/// "today"), or `None` when we're still within the same logical day — the
/// midnight-to-`rollover_hour` window included, so nothing fires there.
fn triage_rollover(
    last_checked: chrono::NaiveDate,
    now: chrono::NaiveDateTime,
    rollover_hour: u32,
) -> Option<chrono::NaiveDate> {
    let day = logical_day(now, rollover_hour);
    (day != last_checked).then_some(day)
}

#[cfg(test)]
mod rollover_tests {
    use super::{logical_day, triage_rollover};

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn d(y: i32, m: u32, day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn before_rollover_hour_is_previous_day() {
        // 05:59 with a 6 AM rollover still belongs to the previous day.
        assert_eq!(logical_day(dt(2026, 7, 11, 5, 59), 6), d(2026, 7, 10));
    }

    #[test]
    fn at_rollover_hour_is_the_new_day() {
        assert_eq!(logical_day(dt(2026, 7, 11, 6, 0), 6), d(2026, 7, 11));
    }

    #[test]
    fn after_rollover_hour_is_the_same_day() {
        assert_eq!(logical_day(dt(2026, 7, 11, 14, 0), 6), d(2026, 7, 11));
    }

    #[test]
    fn just_past_midnight_is_still_the_previous_day() {
        // The whole point: 00:01 is not a new day under a 6 AM rollover.
        assert_eq!(logical_day(dt(2026, 7, 11, 0, 1), 6), d(2026, 7, 10));
    }

    #[test]
    fn zero_rollover_hour_makes_midnight_the_boundary() {
        assert_eq!(logical_day(dt(2026, 7, 10, 23, 59), 0), d(2026, 7, 10));
        assert_eq!(logical_day(dt(2026, 7, 11, 0, 0), 0), d(2026, 7, 11));
    }

    #[test]
    fn out_of_range_hour_falls_back_to_six() {
        // Hour 30 is nonsense; behave exactly like the 6 AM default.
        assert_eq!(logical_day(dt(2026, 7, 11, 5, 0), 30), d(2026, 7, 10));
        assert_eq!(logical_day(dt(2026, 7, 11, 7, 0), 30), d(2026, 7, 11));
    }

    #[test]
    fn rollover_at_exactly_midnight_does_not_fire() {
        // Session last checked July 10; the clock ticks to 00:00 July 11.
        // With a 6 AM rollover this is still "July 10" — no re-check.
        assert_eq!(
            triage_rollover(d(2026, 7, 10), dt(2026, 7, 11, 0, 0), 6),
            None
        );
    }

    #[test]
    fn working_past_midnight_before_rollover_does_not_fire() {
        assert_eq!(
            triage_rollover(d(2026, 7, 10), dt(2026, 7, 11, 2, 30), 6),
            None
        );
    }

    #[test]
    fn same_day_refresh_does_not_fire() {
        assert_eq!(
            triage_rollover(d(2026, 7, 10), dt(2026, 7, 10, 23, 0), 6),
            None
        );
    }

    #[test]
    fn crossing_the_rollover_fires_with_the_new_day() {
        assert_eq!(
            triage_rollover(d(2026, 7, 10), dt(2026, 7, 11, 7, 0), 6),
            Some(d(2026, 7, 11))
        );
    }

    #[test]
    fn first_refresh_after_rollover_but_past_next_midnight_uses_logical_day() {
        // No refresh happened between the 6 AM rollover and 01:00 the next
        // calendar day. The logical day is still July 11 (calendar July 12),
        // so we adopt July 11 — not the calendar date.
        assert_eq!(
            triage_rollover(d(2026, 7, 10), dt(2026, 7, 12, 1, 0), 6),
            Some(d(2026, 7, 11))
        );
    }
}

#[cfg(test)]
mod triage_nudge_tests {
    use super::{Task, triage_nudge_target};

    fn d(y: i32, m: u32, day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// Builds a Morning Triage occurrence with a given id/due/completed state.
    /// Self-contained literal (rather than reusing `task::test_task`, which is
    /// `#[cfg(test)]`-gated to that module) so this test owns its fixtures.
    fn triage(id: &str, due: chrono::NaiveDate, completed: Option<chrono::NaiveDate>) -> Task {
        Task {
            id: id.to_owned(),
            name: "Morning Triage (5mins)".to_owned(),
            types: Vec::new(),
            status: if completed.is_some() { "done" } else { "not_started" }.to_owned(),
            priority: "p1".to_owned(),
            due_date: Some(due),
            hard_deadline: false,
            start_date: None,
            notes: String::new(),
            project: String::new(),
            energy: String::new(),
            context: String::new(),
            estimated_duration: None,
            defer_count: 0,
            last_touched: None,
            see_also: String::new(),
            blocked_by: Vec::new(),
            completed_date: completed,
            linear_issue: String::new(),
        }
    }

    #[test]
    fn fires_when_no_occurrence_completed_today() {
        let today = d(2026, 6, 24);
        // Yesterday's occurrence is done; today's is still open.
        let habits = vec![
            triage("H31", d(2026, 6, 23), Some(d(2026, 6, 23))),
            triage("H41", d(2026, 6, 24), None),
        ];
        let target = triage_nudge_target(&habits, "Morning Triage", today);
        assert_eq!(target.map(|h| h.id.as_str()), Some("H41"));
    }

    #[test]
    fn silent_when_todays_occurrence_completed() {
        let today = d(2026, 6, 24);
        // This is the regression case: today's occurrence (H41) is done,
        // even though a *different* id (H31) was yesterday's. A name match
        // sees the completion; an old fixed-ID check on H31 would not.
        let habits = vec![
            triage("H31", d(2026, 6, 23), Some(d(2026, 6, 23))),
            triage("H41", d(2026, 6, 24), Some(d(2026, 6, 24))),
            triage("H47", d(2026, 6, 25), None),
        ];
        assert!(triage_nudge_target(&habits, "Morning Triage", today).is_none());
    }

    #[test]
    fn case_insensitive_and_tolerates_suffix() {
        let today = d(2026, 6, 24);
        let habits = vec![triage("H41", d(2026, 6, 24), None)];
        // Lowercase pattern still matches "Morning Triage (5mins)".
        assert!(triage_nudge_target(&habits, "morning triage", today).is_some());
    }

    #[test]
    fn empty_pattern_disables_check() {
        let today = d(2026, 6, 24);
        let habits = vec![triage("H41", d(2026, 6, 24), None)];
        assert!(triage_nudge_target(&habits, "  ", today).is_none());
    }

    #[test]
    fn invalid_regex_is_silent() {
        let today = d(2026, 6, 24);
        let habits = vec![triage("H41", d(2026, 6, 24), None)];
        // Unbalanced bracket — must not panic, must not fire.
        assert!(triage_nudge_target(&habits, "Morning [Triage", today).is_none());
    }

    #[test]
    fn no_match_is_silent() {
        let today = d(2026, 6, 24);
        let habits = vec![triage("H41", d(2026, 6, 24), None)];
        assert!(triage_nudge_target(&habits, "Weekly Review", today).is_none());
    }

    #[test]
    fn surfaces_due_today_over_other_matches() {
        let today = d(2026, 6, 24);
        // Out-of-order vec: ensure we pick the due-today row, not the first.
        let habits = vec![
            triage("H47", d(2026, 6, 25), None),
            triage("H41", d(2026, 6, 24), None),
            triage("H31", d(2026, 6, 23), None),
        ];
        let target = triage_nudge_target(&habits, "Morning Triage", today);
        assert_eq!(target.map(|h| h.id.as_str()), Some("H41"));
    }
}
