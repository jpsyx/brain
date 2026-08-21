//! The `tasks` shell: an interactive ratatui frontend — alternate-screen
//! setup, event loop, drawing.
//!
//! Two input modes:
//!   - **Normal**: scroll keys (j/k/d/u/Space/b/g/G), `/` enters search
//!     mode, q quits, Esc clears an active filter (or quits if none).
//!   - **Search mode**: typing edits the query and live-filters the body.
//!     Esc cancels the filter; Enter exits search mode but keeps the
//!     filter so the user can scroll the results. Ctrl-modified chords
//!     (other than Ctrl+C/Ctrl+U) fall through to normal-mode handling so
//!     task shortcuts like Ctrl+Enter still fire on the highlighted row
//!     without leaving `/`.
//!
//! Ctrl+M opens (or focuses) the persistent brain panel — an interactive
//! `claude` PTY rendered via `tui-term` that resumes the shell's
//! most-recently-active session. Alt+L focuses it, Alt+H focuses the tasks
//! panel. When the brain panel is focused, key events are forwarded to the
//! PTY's stdin as raw bytes — Alt+H is the reliable way to pop focus back to
//! tasks from there. (We deliberately avoid a Space leader and Alt+arrow
//! chords: both collide with editing inside Claude's input.) Ctrl+X closes
//! the panel and ends its agent session.
//!
//! Module layout:
//! - This file owns the [`App`] shell type (and `Panel`) so every submodule
//!   can reach its fields; `overlay` owns the exclusive modal state enum and
//!   `modal_state` owns its task-view state structs.
//! - `palette` / `modals` — the command-palette and confirm / brain-input /
//!   help modal state + behavior.
//! - `app_state` / `app_actions` / `app_brain` — the `App` impl, split by
//!   concern.
//! - `event_loop` — terminal setup ([`run_tui`]), the event loop, and overlay
//!   key routing.
//! - `handlers` / `keymap` — per-modal key handlers and the pure key-decision
//!   helpers.
//! - `draw` / `draw_palette` / `draw_modals` / `draw_help` — the rendering of
//!   each surface.
//! - `shell` — the [`ShellRunner`] injection boundary.

mod action;
mod app_actions;
mod app_brain;
mod app_skill_session;
mod app_state;
mod app_sync;
mod draw;
mod draw_assignee;
mod draw_help;
mod draw_modals;
mod draw_palette;
mod draw_sync_log;
mod event_loop;
mod filter_tasks;
mod handlers;
mod keymap;
mod launch;
mod links;
mod logs_view;
mod modal_state;
mod modals;
mod overlay;
mod palette;
pub mod receiver;
mod receiver_state;
mod runtime;
mod search_view;
mod shell;
pub mod singleton;
mod state;
mod status_warning;

#[cfg(test)]
mod tests;

pub(crate) use event_loop::run_tui;
use filter_tasks::filter_tasks;
pub(crate) use launch::TuiLaunch;

// Re-export every submodule's items into the `tui` root so each submodule's
// `use super::*;` can reach its siblings' free functions and shared types
// (the `App` impl is split across files; the handlers / draw / keymap fns
// call across module boundaries). `event_loop` can't be glob-imported because
// its `event_loop` fn would shadow the module name.
pub(crate) use action::*;
pub(crate) use app_state::AppInit;
pub(crate) use app_sync::*;
pub(crate) use draw::*;
pub(crate) use draw_assignee::*;
pub(crate) use draw_help::*;
pub(crate) use draw_modals::*;
pub(crate) use draw_palette::*;
pub(crate) use draw_sync_log::*;
pub(crate) use handlers::*;
pub(crate) use keymap::*;
pub(crate) use links::*;
pub(crate) use logs_view::*;
pub(crate) use modal_state::*;
pub(crate) use overlay::*;
pub(crate) use palette::*;
pub(crate) use search_view::*;
pub(crate) use shell::*;
pub(crate) use state::*;
pub(crate) use status_warning::*;

use std::path::PathBuf;
use std::time::Instant;

use chrono::NaiveDate;
#[cfg(test)]
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::style::Color;

use crate::agent::AgentController;
use crate::config::Config;
use crate::session::AgentKind;
use crate::state::{Db, PanelSide};
use crate::tasks::task::Task;
use crate::tasks::view::View;
use crate::users::UserId;

use shell::ShellRunner;

/// Subtle "elevation" background for the row(s) belonging to the currently
/// selected task.
const SELECTED_BG: Color = Color::Rgb(50, 56, 78);

mod model;

pub(crate) use model::*;

pub(crate) struct App {
    command_context: crate::workspace::CommandContext,
    /// Workspace ingress verified and accepted with this TUI's live lease.
    server_ingress: crate::server::IngressId,
    server_local_capability: crate::server::lifecycle::LeaseId,
    /// Runtime config, held so post-startup actions (the `r`-hotkey triage
    /// re-check) can reach `daily_triage_name_pattern` and
    /// `day_rollover_hour` without re-loading the file.
    config: Config,
    /// Agent frontend running in the brain panel for this shell.
    agent_kind: AgentKind,
    /// Machine-local launch command resolved once with the shell context.
    agent_command: String,
    /// The logical day (see `logical_day`) the daily-triage nudge was last
    /// evaluated for. A refresh only re-runs the check when the current
    /// logical day differs from this, so the modal fires at most once per
    /// day even across a multi-day session.
    triage_day: NaiveDate,
    /// While a startup background sync is in flight, config and task refresh
    /// are deferred so the shell is usable immediately. After refresh, this
    /// remains `Some` only while an outstanding alert waits for the exclusive
    /// overlay slot. Alert evaluation reads the live process-scoped toggle.
    triage_gate: Option<TriageGate>,
    /// Process-scoped opt-out: when true the daily-triage startup nudge is
    /// never evaluated for this run, so the modal can't appear. Seeded from the
    /// portable `enable_daily_triage_check` config; the palette flips it and
    /// writes the config, so the choice outlives the session.
    skip_daily_triage_check: bool,
    /// Path to the tasks CSV, held so palette actions can reload after
    /// mutating it.
    csv_path: PathBuf,
    tasks: TasksState,

    /// The persistent brain panel: an interactive agent PTY. `None` until
    /// the user opens it (Ctrl+M, a brain action, …); once open the layout
    /// splits 50/50 and the panel persists until the user closes it (Ctrl+X /
    /// "Close brain") or the agent exits. Opening it resumes the
    /// most-recently-active free session for the selected frontend, workspace,
    /// actor, and channel (lock + recency, see `state`).
    brain: Option<AgentController>,
    #[cfg(test)]
    brain_transport_override: Option<Box<dyn crate::agent::AgentTransport>>,
    /// Whether the panel has a submitted prompt whose Stop hook has not
    /// completed. Receiver dispatch waits for active work, but can replace an
    /// idle startup panel even while another modal is visible.
    brain_turn_active: bool,

    /// The open skill sessions, each shown as its own brain-panel tab
    /// (`Alt+2`, `Alt+3`, …) while it runs — the builtin daily triage and
    /// whatever the workspace declared in `skill_sessions`. Unlike `brain` these
    /// are never recorded in the session DB (see
    /// `session::env_for_skill_session`), so they are never resumed: if the
    /// shell closes before a run finishes the session is simply lost and the
    /// user starts it again. Each is auto-closed when its run signals completion
    /// (see `crate::skill_session::signal` + `tick_skill_sessions`).
    skill_sessions: Vec<SkillSessionTab>,
    /// The next tab identity to hand out. Monotonic, so a closed tab's id is
    /// never reused within one shell.
    next_session_tab_id: u32,
    /// The workspace's raw `skill_sessions` env value, read once at startup.
    /// Parsed into runnable definitions on demand by
    /// [`crate::skill_session::available`]; `None` when the workspace declares
    /// none.
    configured_skill_sessions: Option<serde_json::Value>,
    #[cfg(test)]
    session_done_url_override: Option<String>,
    #[cfg(test)]
    session_transport_override: Option<Box<dyn crate::agent::AgentTransport>>,

    shell: ShellState,

    /// This shell's lineage id (one per running tasks shell). Owns the lock on
    /// whatever Claude session it's currently driving; the SessionStart hook
    /// attributes session rows to it via `BRAIN_INSTANCE_ID`.
    instance: String,
    /// Machine-local actor resolved once when this shell starts.
    interactive_actor: crate::actor::ActorContext,
    /// Actor immutable for the currently open agent session.
    session_actor: Option<crate::actor::ActorContext>,
    /// The selected workspace root in which the agent panel runs. Used to
    /// resolve that workspace's `.claude/settings.json` and to locate session
    /// transcripts on disk before a `--resume`.
    brain_root: PathBuf,
    /// Path to the state DB, passed down to Claude (via `BRAIN_STATE_DB`) so
    /// the hook writes to the same DB this shell reads.
    db_path: PathBuf,
    /// Always-on run log path. `--verbose` only mirrors detailed diagnostics
    /// to the terminal for non-TUI commands.
    log_path: Option<PathBuf>,
    /// A one-line note shown in the brain footer (e.g. "couldn't find a
    /// session to resume — starting a new chat"). Cleared on the first focus
    /// switch.
    alert: Option<String>,
    /// The shell's one modal slot. Its enum drives both input and drawing, so
    /// simultaneous overlays are unrepresentable.
    overlay: Option<Overlay>,

    /// Transient status line (success / error) shown until the next key
    /// press. Set by palette actions; cleared at the top of the event
    /// loop on the next keystroke.
    flash: Option<FlashKind>,
    /// Receiver configuration warning that remains after transient flashes
    /// clear, so a malformed SMS number cannot silently disable receiving.
    persistent_warning: Option<String>,

    /// Injected runner for the `agenda` zsh function. Boxed so the
    /// production impl can shell out while tests pass a fake that
    /// returns Ok(()) or Err(...) on demand.
    agenda_runner: Box<dyn ShellRunner>,
    /// Injected runner for opening a Linear issue URL in the browser
    /// (`/usr/bin/open <url>`). Same injection rationale as the other
    /// runners — tests pass a fake that records the URL.
    open_runner: Box<dyn ShellRunner>,
    /// SQLite handle shared with the SessionStart hook. Held by `App` for the
    /// lifetime of the tasks shell; tracks which brain session this shell is
    /// driving (lock + recency).
    pub(crate) db: Db,
    /// One owner for receiver-local ingress, intent, session, delivery,
    /// timing, and sync-gate state.
    receiver: crate::tui::receiver::ReceiverRuntime,
    /// App-owned adapter for cross-feature sync observations and effects.
    receiver_sync_runtime: Box<dyn ReceiverSyncRuntime>,
    pub(crate) sync_status: Option<String>,
    pub(crate) sync_status_next_poll: Instant,
    pub(crate) last_seen_downstream_id: Option<i64>,
}

#[cfg(test)]
#[path = "assignment_filter_tests.rs"]
mod assignment_filter_tests;
