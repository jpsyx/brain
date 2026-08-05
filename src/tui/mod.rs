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
//!   can reach its fields; `modal_state` owns the overlay-modal state structs.
//! - `palette` / `modals` — the command-palette and confirm / brain-input /
//!   help modal state + behavior.
//! - `app_state` / `app_actions` / `app_brain` — the `App` impl, split by
//!   concern.
//! - `event_loop` — terminal setup ([`run_tui`]), the event loop, and modal
//!   key routing.
//! - `handlers` / `keymap` — per-modal key handlers and the pure key-decision
//!   helpers.
//! - `draw` / `draw_palette` / `draw_modals` / `draw_help` — the rendering of
//!   each surface.
//! - `shell` — the [`ShellRunner`] injection boundary.

mod app_actions;
mod app_brain;
mod app_state;
mod app_sync;
mod app_triage_tab;
mod draw;
mod draw_assignee;
mod draw_help;
mod draw_modals;
mod draw_palette;
mod event_loop;
mod handlers;
mod keymap;
mod links;
mod logs_view;
mod modal_state;
mod modals;
mod palette;
mod receiver_state;
mod search_view;
mod shell;
pub mod singleton;
mod status_warning;

#[cfg(test)]
mod tests;

pub use event_loop::run_tui;

// Re-export every submodule's items into the `tui` root so each submodule's
// `use super::*;` can reach its siblings' free functions and shared types
// (the `App` impl is split across files; the handlers / draw / keymap fns
// call across module boundaries). `event_loop` can't be glob-imported (its
// `event_loop` fn would shadow the module name), so its shared modal-routing
// types are re-exported by name.
// The modal-routing types are referenced within `event_loop` directly; the
// only out-of-module consumer is the unit-test module, so the re-export is
// test-only.
pub(crate) use draw::*;
pub(crate) use draw_assignee::*;
pub(crate) use draw_help::*;
pub(crate) use draw_modals::*;
pub(crate) use draw_palette::*;
#[cfg(test)]
pub(crate) use event_loop::{ActiveModals, ModalInput, modal_input_target};
pub(crate) use handlers::*;
pub(crate) use keymap::*;
pub(crate) use links::*;
pub(crate) use logs_view::*;
pub(crate) use modal_state::*;
pub(crate) use palette::*;
pub(crate) use search_view::*;
pub(crate) use shell::*;
pub(crate) use status_warning::*;

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use chrono::NaiveDate;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{layout::Rect, style::Color, text::Line};

use crate::agent::AgentController;
use crate::config::Config;
use crate::main_view::MainView;
use crate::session::AgentKind;
use crate::state::{Db, PanelSide};
use crate::tasks::cli::Cli;
use crate::tasks::task::{AssignmentContext, Task};
use crate::tasks::view::View;
use crate::users::UserId;

use shell::ShellRunner;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Tasks,
    Brain,
}

/// Which session is showing inside the brain panel. The panel normally hosts a
/// single persistent session ([`BrainTab::Main`]); [`BrainTab::Triage`] is the
/// ephemeral daily-triage session that appears as a second tab only while a
/// triage pass is running (see `App::triage_brain`). Selected with `Alt+1` /
/// `Alt+2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrainTab {
    Main,
    Triage,
}

/// Subtle "elevation" background for the row(s) belonging to the currently
/// selected task.
const SELECTED_BG: Color = Color::Rgb(50, 56, 78);

/// Deferral state for the startup daily-triage nudge while a background sync is
/// still running. See `App::triage_gate` and `App::tick_triage_gate`.
pub(crate) struct TriageGate {
    /// Newest sync-journal row id when the gate was armed; the gate resolves
    /// once a strictly-newer row appears (a background sync finished). `None`
    /// when the journal was empty at arm time.
    pub(crate) seen_journal_id: Option<i64>,
    /// Next instant we're allowed to poll the journal, to throttle the DB reads
    /// down from the 50ms event-loop tick.
    pub(crate) next_poll: Instant,
}

pub(crate) struct ReceiverSyncGate {
    pub(crate) seen_journal_id: Option<i64>,
    pub(crate) launched_at: Instant,
    pub(crate) next_poll: Instant,
    pub(crate) attempts: u8,
}

pub(crate) struct App<'a> {
    command_context: crate::workspace::CommandContext,
    /// Workspace ingress verified and accepted with this TUI's live lease.
    server_ingress: crate::server::IngressId,
    tag_styles: crate::personalization::tags::TagStyles,
    today: NaiveDate,
    /// Runtime config, held so post-startup actions (the `r`-hotkey triage
    /// re-check) can reach `daily_triage_name_pattern` and
    /// `day_rollover_hour` without re-loading the file.
    config: Config,
    /// Agent frontend running in the brain panel for this shell.
    agent_kind: AgentKind,
    /// The logical day (see `logical_day`) the daily-triage nudge was last
    /// evaluated for. A refresh only re-runs the check when the current
    /// logical day differs from this, so the modal fires at most once per
    /// day even across a multi-day session.
    triage_day: NaiveDate,
    /// While a startup background sync is still in flight, config and task
    /// refresh are deferred so the shell is usable immediately. `Some` means
    /// "waiting for that sync to land". Alert evaluation reads the live
    /// process-scoped toggle only after the refresh succeeds.
    triage_gate: Option<TriageGate>,
    /// Process-scoped opt-out (via `--no-daily-triage-check`): when true the
    /// daily-triage startup nudge is never evaluated for this run, so the modal
    /// can't appear. Not a persistent config change.
    skip_daily_triage_check: bool,
    /// When set (via the `--full-notes` flag), every task starts with its
    /// notes expanded. The per-task `l` toggle still layers on top.
    full_notes: bool,
    /// IDs of tasks/habits whose notes the user has expanded via `l` (or the
    /// "Expand notes" palette action). Effective expansion is
    /// `full_notes || expanded_notes.contains(id)`.
    expanded_notes: HashSet<String>,
    cli: &'a Cli,
    /// Path to the tasks CSV, held so palette actions can reload after
    /// mutating it.
    csv_path: PathBuf,

    /// Full unfiltered task list — needed to rebuild on Tab view-cycle.
    all_tasks: Vec<Task>,
    /// Full unfiltered habit list. Powers `View::Habits` and is reloaded
    /// from `habits.csv` whenever a palette action mutates it.
    all_habits: Vec<Task>,
    /// Current Tab-cycle view. `None` when the initial view was a custom
    /// selector (e.g. `tasks tomorrow`); pressing Tab adopts `View::Today`.
    active_view: Option<View>,

    /// Snapshot of the active view's tasks; the source for in-shell fuzzy filter.
    base_tasks: Vec<Task>,
    /// Pre-rendered top banner. Rebuilt on view-cycle.
    header: Vec<Line<'static>>,

    /// Search mode state (in-shell fuzzy filter).
    query: String,
    in_search: bool,
    matcher: SkimMatcherV2,
    /// Assignment visibility and portable members resolved once at startup.
    assignment: AssignmentContext,
    /// Process-scoped assignee filter selected from the native picker.
    assignment_filter: Option<UserId>,

    /// Currently-visible tasks (after fuzzy filter). Indexed by
    /// `selected_task`; consumed by palette actions that need a task ID.
    visible_tasks: Vec<Task>,
    /// `body_lines` range each visible task occupies (excludes the
    /// trailing blank separator). Used for highlight + scroll-into-view.
    task_line_ranges: Vec<Range<usize>>,
    /// Index into `visible_tasks`. `None` only when the visible list is
    /// empty; otherwise always points at exactly one task.
    selected_task: Option<usize>,
    /// Vim-style numeric count prefix in progress. `Some(n)` after the
    /// user has typed digits (e.g. `3`) but before the motion key that
    /// consumes them (`j`/`k`/↑/↓). Any other keystroke clears it.
    pending_count: Option<usize>,
    /// Body lines for the current `query` (rebuilt on every query change).
    body_lines: Vec<Line<'static>>,
    /// Prefix sum mapping each `body_lines` index to its first visual
    /// (wrapped) row, recomputed each frame from the content width. Length
    /// is `body_lines.len() + 1`; the last entry is the total visual rows.
    /// Everything scroll/highlight-related works in these visual rows so a
    /// note that wraps doesn't desync the selection band from the text.
    visual_row_offsets: Vec<u16>,

    /// Scroll bookkeeping. `scroll` and `last_content_rows` are in visual
    /// (wrapped) rows, matching how `Paragraph::scroll` treats a wrapped
    /// body.
    scroll: u16,
    last_inner_height: u16,
    last_content_rows: u16,

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
    focus: Panel,

    /// The ephemeral daily-triage session, shown as a second brain-panel tab
    /// (`Alt+2`) while a triage pass runs. Unlike `brain` it is never recorded
    /// in the session DB (see `session::env_for_triage`), so it is never
    /// resumed: if the shell closes before triage finishes the session is
    /// simply lost, and the daily-triage nudge fires again next launch. It is
    /// auto-closed when the `/triage` skill signals completion (see
    /// `crate::triage_signal` + `tick_triage_done`).
    triage_brain: Option<AgentController>,
    /// Which brain-panel tab is showing. Only ever `BrainTab::Triage` while
    /// `triage_brain` is `Some`.
    active_brain_tab: BrainTab,
    /// The one-time token brain handed the running triage session in
    /// `BRAIN_TRIAGE_TOKEN`. The completion signal must carry a matching token
    /// to auto-close the tab, so a stale signal from an earlier run can't close
    /// a freshly-opened session.
    triage_token: Option<String>,
    #[cfg(test)]
    triage_done_url_override: Option<String>,
    #[cfg(test)]
    triage_transport_override: Option<Box<dyn crate::agent::AgentTransport>>,

    /// Which main view is showing in the main panel: the tasks view (startup
    /// default) or the brain-directory fuzzy-search view. The brain panel is
    /// app-level and persists across a switch. See `crate::main_view`.
    main_view: MainView,
    /// The currently selected diagnostic log source, when the log main view
    /// is active.
    pub(crate) logs_view: Option<LogsView>,
    /// The brain-directory (fuzzy-search) main view's picker state — entries,
    /// query, matches, and its own palette / confirm overlays. Only receives
    /// keys while `main_view == MainView::BrainSearch` and the main panel is
    /// focused; drawn in the main panel area then.
    search: crate::picker::App,
    /// Which side the brain panel sits on when open (the other side holds the
    /// active main view). Toggled by the brain-search palette's layout row;
    /// persisted in the state DB.
    panel_side: PanelSide,
    /// On-screen rectangle the brain panel last occupied (the right half),
    /// recorded each frame by `draw`. `None` when no brain panel is open.
    /// Read by the mouse handler to decide which panel the wheel scrolls.
    brain_rect: Option<Rect>,

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
    /// Active modal overlay. At most one is open at a time; the event
    /// loop short-circuits to its handler when any is `Some`.
    palette: Option<PaletteState>,
    brain_input: Option<BrainInputState>,
    confirm: Option<ConfirmState>,
    link_picker: Option<LinkPickerState>,
    assignee_filter: Option<AssigneeFilterState>,
    /// Keyboard-shortcuts help modal (opened with `?`).
    help: Option<HelpState>,

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
    /// The optional receiver listener is owned by this TUI and therefore
    /// cannot outlive it. Inbound work waits here until the active agent turn
    /// is safe to switch.
    pub(crate) receiver_server: Option<crate::server::receiver::ReceiverServer>,
    pub(crate) receiver_control: Option<crate::tui::singleton::JobSocket>,
    pub(crate) receiver_rx: Option<Receiver<crate::server::receiver::InboundMessage>>,
    pub(crate) receiver_queue: Vec<crate::server::receiver::InboundMessage>,
    pub(crate) requested_receiver_actor: Option<crate::actor::ActorContext>,
    pub(crate) receiver_lease: Option<receiver_state::Lease>,
    pub(crate) receiver_generation: u64,
    pub(crate) receiver_sender: Option<String>,
    pub(crate) receiver_recipients: Vec<String>,
    pub(crate) receiver_session_id: Option<String>,
    pub(crate) interactive_session_id: Option<String>,
    pub(crate) receiver_resume_session: Option<String>,
    pub(crate) receiver_started: Option<std::time::Instant>,
    pub(crate) receiver_delay_sent: bool,
    pub(crate) receiver_retry_at: Option<std::time::Instant>,
    pub(crate) receiver_sync_gate: Option<ReceiverSyncGate>,
    pub(crate) sync_status: Option<String>,
    pub(crate) sync_status_next_poll: Instant,
}

/// In-shell fuzzy filter: score `tasks` against `query`, keeping matches in
/// descending score order. An empty query returns every task unchanged.
fn filter_tasks<'a>(
    tasks: &'a [Task],
    query: &str,
    assigned_to: Option<&crate::users::UserId>,
    matcher: &SkimMatcherV2,
) -> Vec<&'a Task> {
    let candidates = tasks
        .iter()
        .filter(|task| assigned_to.is_none_or(|user_id| task.assigned_to == user_id.as_str()));
    if query.trim().is_empty() {
        return candidates.collect();
    }
    let mut scored: Vec<(i64, &Task)> = candidates
        .filter_map(|t| {
            let haystack = format!("{} {}", t.id, t.name);
            matcher.fuzzy_match(&haystack, query).map(|s| (s, t))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod assignment_filter_tests {
    use super::{SkimMatcherV2, filter_tasks};
    use crate::tasks::task::test_task;
    use crate::users::UserId;

    #[test]
    fn runtime_assignment_filter_switches_members_and_can_restore_all() {
        let mut pablo = test_task("T1", "not_started");
        pablo.assigned_to = "pablo".to_owned();
        let mut wife = test_task("T2", "not_started");
        wife.assigned_to = "wife".to_owned();
        let tasks = vec![pablo, wife];
        let matcher = SkimMatcherV2::default().ignore_case();
        let pablo_id = UserId::parse("pablo").unwrap();
        let wife_id = UserId::parse("wife").unwrap();

        let pablo_only = filter_tasks(&tasks, "", Some(&pablo_id), &matcher);
        let wife_only = filter_tasks(&tasks, "", Some(&wife_id), &matcher);
        let all = filter_tasks(&tasks, "", None, &matcher);

        assert_eq!(
            pablo_only
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["T1"]
        );
        assert_eq!(
            wife_only
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["T2"]
        );
        assert_eq!(all.len(), 2);
    }
}
