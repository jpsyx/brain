//! The daily-triage nudge: the Yes/Skip handoffs to the brain, the startup +
//! per-refresh check for an uncompleted triage habit, and the pure
//! name-match / logical-day / rollover helpers (with their unit tests).

use regex::RegexBuilder;

use crate::tasks::task::Task;
use crate::tui::*;

struct StartupTriageRefresh {
    config: crate::config::Config,
    tasks: Vec<Task>,
    habits: Vec<Task>,
}

fn refresh_after_successful_startup_sync(
    workspace: &crate::workspace::WorkspaceContext,
) -> anyhow::Result<StartupTriageRefresh> {
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    let config = crate::config::Config::try_load(workspace)?;
    crate::tasks::triage_habits::apply_triage_habits_config_owned(
        workspace,
        config.enable_triage_habits,
        &owner,
    )?;
    let tasks = crate::tasks::task::load_tasks(&workspace.root().join("tasks/tasks.csv"))?;
    let habits = crate::tasks::task::load_habits(&workspace.root().join("tasks/habits.csv"))?;
    Ok(StartupTriageRefresh {
        config,
        tasks,
        habits,
    })
}

impl App<'_> {
    /// Yes-path for the startup daily-triage modal. Opens the daily-triage pass
    /// in its own ephemeral brain-panel tab (`Alt+2`) seeded with `/triage`, so
    /// the (often long, often interactive) pass runs in the background and the
    /// main session (`Alt+1`) stays free. See `open_triage_tab`.
    pub(crate) fn run_triage(&mut self) {
        self.open_triage_tab();
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
        if let Some(habit) = triage_modal_target(
            self.config.enable_triage_habits,
            self.skip_daily_triage_check,
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

    /// Defer the startup daily-triage nudge until a background sync lands.
    ///
    /// Called by `run_tui` *instead of* the immediate `check_daily_triage` when
    /// a startup sync is in flight: the shell stays usable with no modal, and
    /// the nudge is only evaluated once the sync has updated `habits.csv`.
    /// `seen_journal_id` is the newest journal row at arm time. The gate stays
    /// closed until a newer row proves that the startup sync completed.
    pub(crate) fn arm_triage_gate(
        &mut self,
        seen_journal_id: Option<i64>,
        now: std::time::Instant,
    ) {
        self.triage_gate = Some(TriageGate {
            seen_journal_id,
            // Allow an immediate first poll (a very fast sync may already be done).
            next_poll: now,
        });
    }

    /// One event-loop tick of the deferred triage gate.
    ///
    /// No-op unless a gate is armed. Once the background sync has landed (a
    /// newer journal row), it reloads the CSVs (so the nudge sees the synced
    /// completion state) and runs the normal
    /// `check_daily_triage` exactly once — which shows the modal only if triage
    /// is *still* incomplete for today. Journal polling is throttled off the
    /// 50ms loop via `next_poll`.
    pub(crate) fn tick_triage_gate(&mut self) {
        let Some(gate) = self.triage_gate.as_ref() else {
            return;
        };
        let now = std::time::Instant::now();
        if now < gate.next_poll {
            return;
        }
        let latest = crate::sync::journal::Journal::open(
            &self.command_context.workspace.paths().sync_journal(),
        )
        .ok()
        .and_then(|j| j.latest_successful_downstream_id().ok())
        .flatten();
        if triage_gate_resolved(gate.seen_journal_id, latest) {
            self.triage_gate = None;
            match refresh_after_successful_startup_sync(&self.command_context.workspace) {
                Ok(refreshed) => {
                    self.config = refreshed.config;
                    self.all_tasks = refreshed.tasks;
                    self.all_habits = refreshed.habits;
                    let selector = self
                        .active_view
                        .map_or(crate::tasks::selector::Selector::All, |view| {
                            view.selector(self.today)
                        });
                    let spec = crate::tasks::view::build_view(
                        self.cli,
                        &selector,
                        self.active_view,
                        self.data_for_view(self.active_view),
                        self.today,
                    );
                    self.header =
                        crate::tasks::render::header_lines(&spec, self.cli, self.active_view);
                    self.base_tasks = spec.tasks;
                    self.rebuild_body();
                    if should_check_daily_triage(
                        TriageAlertEvent::RefreshSucceeded,
                        false,
                        self.skip_daily_triage_check,
                    ) {
                        self.check_daily_triage();
                    }
                }
                Err(error) => {
                    crate::logging::log(format!("post-sync triage refresh failed: {error:#}"));
                    self.flash = Some(FlashKind::Error(format!(
                        "post-sync task refresh failed: {error}"
                    )));
                }
            }
        } else if let Some(gate) = self.triage_gate.as_mut() {
            gate.next_poll = now + std::time::Duration::from_millis(500);
        }
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

/// Whether the deferred triage gate should resolve now.
///
/// Resolves only when a strictly-newer sync-journal row exists than the one
/// seen at arm time. A timeout must not resolve this gate because that would
/// evaluate stale local habits while the startup sync is still running or has
/// failed. Pure so the decision is unit-tested without a clock or a DB.
fn triage_gate_resolved(seen: Option<i64>, latest: Option<i64>) -> bool {
    match (latest, seen) {
        (Some(l), Some(s)) => l > s,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TriageAlertEvent {
    PaletteEnabled,
    RefreshSucceeded,
}

/// Decide whether an alert check runs now or remains deferred to fresh state.
pub(super) fn should_check_daily_triage(
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

/// Gate the daily-triage nudge on the process-scoped `--no-daily-triage-check`
/// opt-out before consulting [`triage_nudge_target`]. When `disabled` is set the
/// modal never fires this run regardless of habit state; this is a per-process
/// flag, not a persistent config change. Pure so the opt-out is unit-tested
/// without constructing an `App`.
fn triage_modal_target<'h>(
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
fn triage_rollover(
    last_checked: chrono::NaiveDate,
    now: chrono::NaiveDateTime,
    rollover_hour: u32,
) -> Option<chrono::NaiveDate> {
    let day = logical_day(now, rollover_hour);
    (day != last_checked).then_some(day)
}

#[cfg(test)]
mod triage_gate_tests {
    use super::{
        TriageAlertEvent, refresh_after_successful_startup_sync, should_check_daily_triage,
        triage_gate_resolved,
    };

    #[test]
    fn palette_reenable_defers_until_refresh_then_uses_live_alert_state() {
        assert!(!should_check_daily_triage(
            TriageAlertEvent::PaletteEnabled,
            true,
            false,
        ));
        assert!(should_check_daily_triage(
            TriageAlertEvent::RefreshSucceeded,
            false,
            false,
        ));
        assert!(!should_check_daily_triage(
            TriageAlertEvent::RefreshSucceeded,
            false,
            true,
        ));
    }

    #[test]
    fn resolves_when_a_newer_journal_row_appears() {
        // Same id → sync hasn't finished yet.
        assert!(!triage_gate_resolved(Some(5), Some(5)));
        // A newer row → a sync completed.
        assert!(triage_gate_resolved(Some(5), Some(6)));
    }

    #[test]
    fn first_ever_row_resolves_from_an_empty_journal() {
        assert!(triage_gate_resolved(None, Some(1)));
        assert!(!triage_gate_resolved(None, None));
    }

    #[test]
    fn does_not_resolve_at_the_deadline_without_a_completed_sync() {
        // A slow or offline sync must not make the gate evaluate stale local
        // habits. It remains closed until a newer journal row proves that a
        // sync completed.
        assert!(!triage_gate_resolved(Some(5), Some(5)));
        assert!(!triage_gate_resolved(None, None));
    }

    #[test]
    fn successful_startup_sync_reloads_opposite_portable_config_and_task_state() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".config/config.json"),
            b"{\"enable_triage_habits\":false}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/habits.csv"),
            b"task_uuid,task_id,task_name,status,assigned_to,system_key\n8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,not_started,member,brain.triage.daily\n",
        ).unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
            crate::workspace::WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();

        let refreshed = refresh_after_successful_startup_sync(&workspace).unwrap();

        assert!(!refreshed.config.enable_triage_habits);
        assert!(
            refreshed
                .habits
                .iter()
                .all(|habit| !habit.is_managed_triage())
        );
        assert!(
            crate::tasks::task::load_habits(&root.join("tasks/habits.csv"))
                .unwrap()
                .iter()
                .all(|habit| !habit.is_managed_triage())
        );
    }

    #[test]
    fn successful_startup_sync_reads_config_after_acquiring_the_task_store_owner() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join(".config/config.json"),
            b"{\"enable_triage_habits\":true}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/habits.csv"),
            b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::parse("a09c6257-6ccc-4a39-97a4-058c73a8c569").unwrap(),
            crate::workspace::WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();
        let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&workspace).unwrap();
        let refresh_workspace = workspace;
        let refresh = std::thread::spawn(move || {
            refresh_after_successful_startup_sync(&refresh_workspace).unwrap()
        });

        std::thread::sleep(std::time::Duration::from_millis(100));
        std::fs::write(
            root.join(".config/config.json"),
            b"{\"enable_triage_habits\":false}\n",
        )
        .unwrap();
        drop(owner);

        let refreshed = refresh.join().unwrap();
        assert!(!refreshed.config.enable_triage_habits);
        assert!(
            refreshed
                .habits
                .iter()
                .all(|habit| !habit.is_managed_triage())
        );
    }
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
            task_uuid: None,
            id: id.to_owned(),
            name: "Morning Triage (5mins)".to_owned(),
            types: Vec::new(),
            status: if completed.is_some() {
                "done"
            } else {
                "not_started"
            }
            .to_owned(),
            priority: "p1".to_owned(),
            due_date: Some(due),
            hard_deadline: false,
            start_date: None,
            assigned_to: String::new(),
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
            system_key: String::new(),
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
    fn disabled_flag_suppresses_the_nudge() {
        use super::triage_modal_target;
        let today = d(2026, 6, 24);
        // An open occurrence due today would normally fire the modal.
        let habits = vec![triage("H41", d(2026, 6, 24), None)];
        assert!(triage_modal_target(true, false, &habits, "Morning Triage", today).is_some());
        // The process-scoped `--no-daily-triage-check` opt-out suppresses it.
        assert!(triage_modal_target(true, true, &habits, "Morning Triage", today).is_none());
        // The portable feature flag wins over every process preference.
        assert!(triage_modal_target(false, false, &habits, "Morning Triage", today).is_none());
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
