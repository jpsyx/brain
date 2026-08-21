//! The daily-triage nudge: the Yes/Skip handoffs to the brain, the startup +
//! per-refresh check for an uncompleted triage habit, and the pure
//! name-match / logical-day / rollover helpers (with their unit tests).

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
    reinstall_lifecycle_after_pull(workspace);
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

/// Re-assert this machine's lifecycle artifacts after a pull landed.
///
/// `.claude/settings.json` and the bridge scripts live inside the workspace
/// root and are not excluded from sync, so a machine still running an older
/// brain re-publishes its own versions and a pull hands them to everyone else.
/// Startup installs them *before* the startup pull, so without this the pull
/// silently wins and this machine runs with another machine's hook commands —
/// which is how a fixed hook path came back broken, complete with the remote's
/// older mtime.
///
/// These artifacts are generated from the running binary, never authored, so
/// the local binary is authoritative and reinstalling is the resolution.
/// Installation only writes on a real difference, so the common case where the
/// pull changed nothing touches no mtimes and triggers no push.
fn reinstall_lifecycle_after_pull(workspace: &crate::workspace::WorkspaceContext) {
    if let Err(error) = crate::command::server::refresh_agent_hooks(workspace.root()) {
        crate::logging::log(format!(
            "reinstalling lifecycle artifacts after a pull failed: {error:#}"
        ));
    }
}

impl App {
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
            open_overlay(
                &mut self.overlay,
                Overlay::TaskConfirmation(ConfirmState::run_triage(
                    habit.id.clone(),
                    habit.name.clone(),
                )),
            );
        }
    }

    /// Bring the on-screen daily-triage nudge in line with post-sync state.
    ///
    /// Counterpart to [`Self::check_daily_triage`] for the moment *after* a
    /// startup sync lands: the nudge may already be up (raised before the sync
    /// finished), and the synced habits may now show that triage was completed on
    /// another machine.
    pub(crate) fn reconcile_daily_triage_alert(&mut self) -> TriageAlertResolution {
        let target = triage_modal_target(
            self.config.enable_triage_habits,
            self.skip_daily_triage_check,
            &self.all_habits,
            &self.config.daily_triage_name_pattern,
            self.today,
        );
        let occupancy = match self.overlay.as_ref() {
            Some(Overlay::TaskConfirmation(confirm)) if confirm.kind == ConfirmKind::RunTriage => {
                TriageAlertOccupancy::TriageNudge
            }
            Some(_) => TriageAlertOccupancy::OtherOverlay,
            None => TriageAlertOccupancy::Empty,
        };
        let resolution = resolve_triage_alert(target.is_some(), occupancy);
        match resolution {
            TriageAlertResolution::Open => {
                if let Some(habit) = target {
                    open_overlay(
                        &mut self.overlay,
                        Overlay::TaskConfirmation(ConfirmState::run_triage(
                            habit.id.clone(),
                            habit.name.clone(),
                        )),
                    );
                }
            }
            TriageAlertResolution::Dismiss => {
                crate::logging::log("daily triage nudge withdrawn: sync showed it already done");
                close_overlay(&mut self.overlay);
                self.flash = Some(FlashKind::Info(
                    "daily triage was already done on another machine".to_owned(),
                ));
            }
            TriageAlertResolution::Defer | TriageAlertResolution::Leave => {}
        }
        resolution
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
            refresh_complete: false,
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
        let Some((seen_journal_id, next_poll, refresh_complete)) = self
            .triage_gate
            .as_ref()
            .map(|gate| (gate.seen_journal_id, gate.next_poll, gate.refresh_complete))
        else {
            return;
        };
        if refresh_complete {
            let pending = should_check_daily_triage(
                TriageAlertEvent::RefreshSucceeded,
                false,
                self.skip_daily_triage_check,
            ) && triage_reconciliation_pending(self.reconcile_daily_triage_alert());
            if !pending {
                self.triage_gate = None;
            }
            return;
        }
        let now = std::time::Instant::now();
        if now < next_poll {
            return;
        }
        let latest = crate::sync::journal::Journal::open(
            &self.command_context.workspace.paths().sync_journal(),
        )
        .ok()
        .and_then(|j| j.latest_successful_downstream_id().ok())
        .flatten();
        if triage_gate_resolved(seen_journal_id, latest) {
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
                        &self.task_options,
                        &selector,
                        self.active_view,
                        self.data_for_view(self.active_view),
                        self.today,
                    );
                    self.header = crate::tasks::render::header_lines(
                        &spec,
                        &self.task_options,
                        self.active_view,
                    );
                    self.base_tasks = spec.tasks;
                    self.rebuild_body();
                    // The nudge was already raised at startup, so this is a
                    // reconciliation, not a first look: open it if the synced
                    // state now says triage is outstanding, and withdraw a stale
                    // one if the sync proved it was already done elsewhere.
                    let pending =
                        should_check_daily_triage(
                            TriageAlertEvent::RefreshSucceeded,
                            false,
                            self.skip_daily_triage_check,
                        ) && triage_reconciliation_pending(self.reconcile_daily_triage_alert());
                    if pending {
                        if let Some(gate) = self.triage_gate.as_mut() {
                            gate.refresh_complete = true;
                        }
                    } else {
                        self.triage_gate = None;
                    }
                }
                Err(error) => {
                    self.triage_gate = None;
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

mod decision;

use decision::*;
pub(super) use decision::{TriageAlertEvent, should_check_daily_triage};

#[cfg(test)]
#[path = "triage_rollover_tests.rs"]
mod rollover_tests;
#[cfg(test)]
#[path = "triage_gate_tests.rs"]
mod triage_gate_tests;
#[cfg(test)]
#[path = "triage_nudge_tests.rs"]
mod triage_nudge_tests;
