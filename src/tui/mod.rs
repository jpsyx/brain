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
//! the panel and ends its claude session.
//!
//! Module layout:
//! - This file owns the shared data types ([`App`] and the modal-state
//!   structs) so every submodule can reach their fields.
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
mod draw;
mod draw_help;
mod draw_modals;
mod draw_palette;
mod event_loop;
mod handlers;
mod keymap;
mod links;
mod modals;
mod palette;
mod search_view;
mod shell;

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
#[cfg(test)]
pub(crate) use event_loop::{ActiveModals, ModalInput, modal_input_target};
#[cfg(test)]
pub(crate) use app_brain::advance_submit_countdown;
pub(crate) use draw::*;
pub(crate) use draw_help::*;
pub(crate) use draw_modals::*;
pub(crate) use draw_palette::*;
pub(crate) use handlers::*;
pub(crate) use keymap::*;
pub(crate) use links::*;
pub(crate) use palette::*;
pub(crate) use search_view::*;
pub(crate) use shell::*;

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use chrono::NaiveDate;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{layout::Rect, style::Color, text::Line};

use crate::config::Config;
use crate::main_view::MainView;
use crate::pty_pane::PtyPane;
use crate::state::{Db, PanelSide};
use crate::tasks::cli::Cli;
use crate::tasks::task::Task;
use crate::tasks::view::View;

use shell::ShellRunner;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Tasks,
    Brain,
}

/// Subtle "elevation" background for the row(s) belonging to the currently
/// selected task.
const SELECTED_BG: Color = Color::Rgb(50, 56, 78);

/// One row in the command palette. See `palette` for the command table.
pub(crate) struct PaletteState {
    filter: String,
    selected: usize,
    /// ID of the currently-selected task / habit at the moment the
    /// palette was opened, if any. Drives the task actions modal title ("Task
    /// T123 actions") AND the labels of task-specific commands when
    /// shown in the global command palette ("Defer T123 +1d").
    task_id: Option<String>,
    /// Task name captured at open time. Shown as a dim subtitle in the
    /// task actions modal so the user can sanity-check what they're about
    /// to act on. Unused in the global command palette (task IDs already appear
    /// in command labels there).
    task_label: Option<String>,
    /// Whether the in-context selection is a habit (id starts with `H`).
    /// Task-specific commands with `works_on_habits: false` are hidden
    /// for habits.
    context_is_habit: bool,
    /// Whether the in-context selection has notes. The "Expand/Collapse
    /// notes" command is hidden when false.
    context_has_notes: bool,
    /// Whether the in-context selection's notes are currently expanded.
    /// Drives the toggle command's label (Expand vs Collapse).
    context_notes_expanded: bool,
    /// The in-context selection's link situation (Linear issue and/or notes
    /// URLs). The "open link" command is hidden when `LinkKind::None` and its
    /// label is chosen from this.
    context_links: LinkKind,
    /// When true, hide global commands so only task-scoped actions show.
    /// Set by Enter-on-task to give a focused task actions modal.
    task_actions_modal: bool,
    /// Whether the brain panel is currently open. Gates the "Close brain"
    /// command — there's nothing to close when no panel is up.
    brain_open: bool,
}

/// Visual intent of a confirm modal — drives the accent (border, title,
/// focused button) and so signals whether the action is constructive or
/// destructive. `Success` is green (e.g. mark-complete), `Danger` is red
/// (e.g. remove).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ConfirmIntent {
    Success,
    Danger,
}

impl ConfirmIntent {
    /// Accent color for the modal chrome.
    pub(crate) const fn accent(self) -> Color {
        match self {
            // Green — the same accent used for success flashes.
            Self::Success => Color::Rgb(158, 206, 106),
            // Pink-red — destructive.
            Self::Danger => Color::Rgb(247, 118, 142),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ConfirmKind {
    MarkComplete,
    Remove,
    /// Triggered when Ctrl+A finds no agenda for today (the `agenda`
    /// helper exited non-zero). Yes path spawns the brain panel with a
    /// "generate today's agenda" prompt.
    GenerateAgenda,
    /// Triggered at tasks-shell startup when the configured daily-triage habit
    /// has not been completed today. Yes path spawns the brain panel
    /// with a `/triage` prompt; the Skip path tells the brain to skip
    /// triage for the day (see [`ConfirmChoice::Skip`]).
    RunTriage,
}

/// A button in the confirm modal. Every modal has `Yes` / `No`; only the
/// [`ConfirmKind::RunTriage`] modal additionally offers `Skip`, which
/// hands off to the brain with the documented "skip daily triage" prompt
/// so today's triage habit is marked done without running a pass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ConfirmChoice {
    Yes,
    No,
    Skip,
}

/// Prompt sent to the brain panel when the user picks **Skip** on the
/// daily-triage modal. It uses the `/triage` + `/todo` skills' documented
/// skip trigger ("skip daily triage") so the brain marks today's Morning
/// Triage habit done and runs no triage pass.
pub(crate) const SKIP_TRIAGE_PROMPT: &str = "Skip daily triage today. Per the triage skill's \
skip rule, mark today's Morning Triage habit done (mark_done.py) and run nothing else.";

/// State for the confirmation modal. Most modals are Yes/No; the
/// daily-triage modal also offers Skip (see [`ConfirmState::choices`]).
/// Bound to a specific task at open time so subsequent navigation can't
/// change what the confirmation actually operates on.
pub(crate) struct ConfirmState {
    /// Which action this confirmation gates. Determines what runs on Yes.
    kind: ConfirmKind,
    /// Whether this confirmation is constructive (`Success`, green) or
    /// destructive (`Danger`, red). Drives the modal accent.
    intent: ConfirmIntent,
    /// Modal title (rendered in the block border), e.g. "Confirm" or
    /// "Remove T123".
    title: String,
    /// Body line shown above the buttons, e.g. "Mark T123 as complete?".
    prompt: String,
    /// Task ID this confirmation operates on. Captured at construction.
    task_id: String,
    /// Task name, shown in a dimmer second line so the user can sanity-
    /// check what they're about to mutate without context-switching to
    /// the list behind the modal.
    task_label: String,
    /// Which button is currently focused. Defaults to `Yes` since the user
    /// explicitly invoked the action — they want to confirm, not back out,
    /// in the common case. Movement is constrained to `self.choices()`.
    focus: ConfirmChoice,
}

/// State for the brain-input modal. The buffer is the raw user text; when
/// `about_task` is set, the message that's actually sent to `brain msg`
/// is prefixed with "This message is about <ID>: " so the brain agent
/// has clear context on which task the user is asking about. `task_label`
/// is set in lockstep with `about_task` and is shown as a dim subtitle
/// in the modal so the user can sanity-check the target.
pub(crate) struct BrainInputState {
    buffer: String,
    about_task: Option<String>,
    task_label: Option<String>,
}

pub(crate) enum FlashKind {
    Info(String),
    Error(String),
}

/// State for the keyboard-shortcuts help modal (opened with `?`). Just a
/// scroll offset — the content is rendered straight off `shortcuts::ALL`.
pub(crate) struct HelpState {
    pub(crate) scroll: u16,
}

/// State for the link-picker modal. Opened by the Ctrl+O "open" action (or
/// the "open link" palette command) when an entry resolves to more than
/// one openable link — the Linear issue plus one or more URLs in its notes.
/// A single link bypasses the modal and opens directly. Bound to the task's
/// id at open time so later navigation can't change what it operates on.
pub(crate) struct LinkPickerState {
    /// The task whose links these are; shown in the modal title.
    task_id: String,
    /// Openable links, Linear first (see `task_links`). Always ≥ 2 when the
    /// modal is shown.
    links: Vec<Link>,
    /// Highlighted row.
    selected: usize,
}

pub(crate) struct App<'a> {
    today: NaiveDate,
    /// Runtime config, held so post-startup actions (the `r`-hotkey triage
    /// re-check) can reach `daily_triage_name_pattern` and
    /// `day_rollover_hour` without re-loading the file.
    config: Config,
    /// The logical day (see `logical_day`) the daily-triage nudge was last
    /// evaluated for. A refresh only re-runs the check when the current
    /// logical day differs from this, so the modal fires at most once per
    /// day even across a multi-day session.
    triage_day: NaiveDate,
    /// When set (via the `--full-notes` flag), every task starts with its
    /// notes expanded. The per-task `l` toggle still layers on top.
    full_notes: bool,
    /// IDs of tasks/habits whose notes the user has expanded via `l` (or the
    /// "Expand notes" palette action). Effective expansion is
    /// `full_notes || expanded_notes.contains(id)`.
    expanded_notes: HashSet<String>,
    cli: &'a Cli,
    /// Path to the tasks CSV — held so palette actions can reload after
    /// mutating it externally (e.g. `mark_done.py`).
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

    /// The persistent brain panel: an interactive `claude` PTY. `None` until
    /// the user opens it (Ctrl+M, a brain action, …); once open the layout
    /// splits 50/50 and the panel persists until the user closes it (Ctrl+X /
    /// "Close brain") or claude exits. Opening it resumes the
    /// most-recently-active free session (lock + recency, see `state`), so the
    /// conversation survives closing the panel and quitting the shell.
    brain: Option<PtyPane>,
    focus: Panel,

    /// Which main view is showing in the main panel: the tasks view (startup
    /// default) or the brain-directory fuzzy-search view. The brain panel is
    /// app-level and persists across a switch. See `crate::main_view`.
    main_view: MainView,
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
    /// whatever brain session it's currently driving; the SessionStart hook
    /// attributes session rows to it via `TASKS_INSTANCE_ID`.
    instance: String,
    /// The directory claude runs in for the brain panel (`~/brain`). Used to
    /// resolve the SessionStart hook's `.claude/settings.json` and to locate
    /// session transcripts on disk before a `--resume`.
    brain_root: PathBuf,
    /// Path to the state DB, passed down to claude (via `TASKS_STATE_DB`) so
    /// the hook writes to the same DB this shell reads.
    db_path: PathBuf,
    /// A one-line note shown in the brain footer (e.g. "couldn't find a
    /// session to resume — starting a new chat"). Cleared on the first focus
    /// switch.
    alert: Option<String>,
    /// Event-loop ticks left before the deferred submitting `Return` is sent
    /// to the brain PTY. Set when a prompt is seeded into an already-open panel
    /// (Ctrl+Shift+M, Defer/Start/Remove while the panel is up, …); counted
    /// down each tick by `tick_brain_submit`. `0` means nothing is pending.
    pending_brain_submit: u8,

    /// Active modal overlay. At most one is open at a time; the event
    /// loop short-circuits to its handler when any is `Some`.
    palette: Option<PaletteState>,
    brain_input: Option<BrainInputState>,
    confirm: Option<ConfirmState>,
    link_picker: Option<LinkPickerState>,
    /// Keyboard-shortcuts help modal (opened with `?`).
    help: Option<HelpState>,

    /// Transient status line (success / error) shown until the next key
    /// press. Set by palette actions; cleared at the top of the event
    /// loop on the next keystroke.
    flash: Option<FlashKind>,

    /// Injected runner for the `agenda` zsh function. Boxed so the
    /// production impl can shell out while tests pass a fake that
    /// returns Ok(()) or Err(...) on demand.
    agenda_runner: Box<dyn ShellRunner>,
    /// Injected runner for the `habits` zsh function. Same rationale
    /// as `agenda_runner`.
    habits_runner: Box<dyn ShellRunner>,
    /// Injected runner for opening a Linear issue URL in the browser
    /// (`/usr/bin/open <url>`). Same injection rationale as the other
    /// runners — tests pass a fake that records the URL.
    open_runner: Box<dyn ShellRunner>,
    /// SQLite handle shared with the SessionStart hook. Held by `App` for the
    /// lifetime of the tasks shell; tracks which brain session this shell is
    /// driving (lock + recency).
    pub(crate) db: Db,
}

/// In-shell fuzzy filter: score `tasks` against `query`, keeping matches in
/// descending score order. An empty query returns every task unchanged.
fn filter_tasks<'a>(tasks: &'a [Task], query: &str, matcher: &SkimMatcherV2) -> Vec<&'a Task> {
    if query.trim().is_empty() {
        return tasks.iter().collect();
    }
    let mut scored: Vec<(i64, &Task)> = tasks
        .iter()
        .filter_map(|t| {
            let haystack = format!("{} {}", t.id, t.name);
            matcher.fuzzy_match(&haystack, query).map(|s| (s, t))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, t)| t).collect()
}
