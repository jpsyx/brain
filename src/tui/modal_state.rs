//! State for the overlay modals the shell can raise over its panels: the
//! command palette, the confirm dialog (with its intent/kind/choice enums),
//! the brain-input composer, the transient flash line, the help scroller, and
//! the link picker. The `App` shell state itself lives in the `tui` root.
//!
//! Fields are `pub(super)` (visible to `tui` and its submodules) so the
//! per-modal key handlers, constructors, and draw code — all under `tui` —
//! can reach them, without widening the surface to the whole crate.

use ratatui::style::Color;
use std::path::PathBuf;

use crate::tui::{Link, LinkKind};

/// One row in the command palette. See `palette` for the command table.
pub(crate) struct PaletteState {
    pub(super) filter: String,
    pub(super) selected: usize,
    /// ID of the currently-selected task / habit at the moment the
    /// palette was opened, if any. Drives the task actions modal title ("Task
    /// T123 actions") AND the labels of task-specific commands when
    /// shown in the global command palette ("Defer T123 +1d").
    pub(super) task_id: Option<String>,
    /// Task name captured at open time. Shown as a dim subtitle in the
    /// task actions modal so the user can sanity-check what they're about
    /// to act on. Unused in the global command palette (task IDs already appear
    /// in command labels there).
    pub(super) task_label: Option<String>,
    /// Whether the in-context selection is a habit (id starts with `H`).
    /// Task-specific commands with `works_on_habits: false` are hidden
    /// for habits.
    pub(super) context_is_habit: bool,
    /// Whether the in-context selection has notes. The "Expand/Collapse
    /// notes" command is hidden when false.
    pub(super) context_has_notes: bool,
    /// Whether the in-context selection's notes are currently expanded.
    /// Drives the toggle command's label (Expand vs Collapse).
    pub(super) context_notes_expanded: bool,
    /// The in-context selection's link situation (Linear issue and/or notes
    /// URLs). The "open link" command is hidden when `LinkKind::None` and its
    /// label is chosen from this.
    pub(super) context_links: LinkKind,
    /// When true, hide global commands so only task-scoped actions show.
    /// Set by Enter-on-task to give a focused task actions modal.
    pub(super) task_actions_modal: bool,
    /// Whether the brain panel is currently open. Gates the "Close brain"
    /// command — there's nothing to close when no panel is up.
    pub(super) brain_open: bool,
    /// Whether this verbose run has a log file the TUI can reveal.
    pub(super) logs_available: bool,
    pub(super) messaging_server_running: bool,
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
    /// Triggered by the "Show logs" command palette row in verbose TUI runs.
    /// Yes path reveals the log directory and opens the log file.
    ShowLogs,
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
skip rule, mark today's Morning Triage habit done and run nothing else.";

/// State for the confirmation modal. Most modals are Yes/No; the
/// daily-triage modal also offers Skip (see [`ConfirmState::choices`]).
/// Bound to a specific task at open time so subsequent navigation can't
/// change what the confirmation actually operates on.
pub(crate) struct ConfirmState {
    /// Which action this confirmation gates. Determines what runs on Yes.
    pub(super) kind: ConfirmKind,
    /// Whether this confirmation is constructive (`Success`, green) or
    /// destructive (`Danger`, red). Drives the modal accent.
    pub(super) intent: ConfirmIntent,
    /// Modal title (rendered in the block border), e.g. "Confirm" or
    /// "Remove T123".
    pub(super) title: String,
    /// Body line shown above the buttons, e.g. "Mark T123 as complete?".
    pub(super) prompt: String,
    /// Task ID this confirmation operates on. Captured at construction.
    pub(super) task_id: String,
    /// Task name, shown in a dimmer second line so the user can sanity-
    /// check what they're about to mutate without context-switching to
    /// the list behind the modal.
    pub(super) task_label: String,
    /// Optional filesystem path carried by non-task confirmations.
    pub(super) path: Option<PathBuf>,
    /// Which button is currently focused. Defaults to `Yes` since the user
    /// explicitly invoked the action — they want to confirm, not back out,
    /// in the common case. Movement is constrained to `self.choices()`.
    pub(super) focus: ConfirmChoice,
}

/// State for the brain-input modal. The buffer is the raw user text; when
/// `about_task` is set, the message that's actually sent to `brain msg`
/// is prefixed with "This message is about <ID>: " so the brain agent
/// has clear context on which task the user is asking about. `task_label`
/// is set in lockstep with `about_task` and is shown as a dim subtitle
/// in the modal so the user can sanity-check the target.
pub(crate) struct BrainInputState {
    pub(super) buffer: String,
    pub(super) about_task: Option<String>,
    pub(super) task_label: Option<String>,
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
    pub(super) task_id: String,
    /// Openable links, Linear first (see `task_links`). Always ≥ 2 when the
    /// modal is shown.
    pub(super) links: Vec<Link>,
    /// Highlighted row.
    pub(super) selected: usize,
}
