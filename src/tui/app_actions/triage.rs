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

    /// Skip-path for the startup daily-triage modal. Skipping daily triage is
    /// deterministic — it only marks today's protected Morning Triage
    /// occurrence done and spawns the next — so this runs the native
    /// completion in-process (the same mutation as
    /// `brain habits complete-managed-triage daily`) rather than round-tripping
    /// through the brain panel. No agent, no prompt. Respects
    /// `enable_triage_habits`: a disabled feature is a no-op that still
    /// dismisses the nudge. Contrast the Yes path (`run_triage`) and agenda
    /// generation, which are agent-driven because they involve judgement.
    pub(crate) fn skip_triage(&mut self) {
        let outcome = crate::tasks::triage_habits::complete_managed_triage(
            &self.command_context.workspace,
            crate::tasks::triage_habits::ManagedTriageKind::Daily,
            self.config.enable_triage_habits,
            self.today,
        );
        match outcome {
            Ok(_) => {
                if let Err(error) = self.reload_tasks() {
                    crate::logging::log(format!("reload after triage skip failed: {error:#}"));
                }
                self.flash = Some(FlashKind::Info("✓ daily triage skipped".to_owned()));
            }
            Err(error) => {
                crate::logging::log(format!("triage skip failed: {error:#}"));
                self.flash = Some(FlashKind::Error(format!("triage skip failed: {error}")));
            }
        }
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
#[path = "triage_rollover_tests.rs"]
mod rollover_tests;
#[cfg(test)]
#[path = "triage_gate_tests.rs"]
mod triage_gate_tests;
#[cfg(test)]
#[path = "triage_nudge_tests.rs"]
mod triage_nudge_tests;
