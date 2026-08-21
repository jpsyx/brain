//! The command-palette model: the action enum, the per-command scope/flags,
//! the direct-key shortcut map, and the ordered command table.

use crate::skill_session::SkillSessionKey;
use crate::tui::{LinkKind, PaletteState};

/// A per-command visibility predicate: given the current palette state (a
/// snapshot of the TUI state relevant to the palette), decide whether the
/// command should appear. This is where each command's *conditional* visibility
/// lives (e.g. "Close brain" only with a panel open, the triage-tab switches
/// only while a triage session runs), on top of the structural `scope` /
/// `works_on_habits` gates. Plain `fn` pointers so the table stays `const`.
pub(super) type VisibleWhen = fn(&PaletteState) -> bool;

/// One rendered palette row: its stable 1-based number, its resolved label, the
/// action it fires, and its direct-key hint.
///
/// Rows are *owned* rather than `&'static PaletteCommand` because not every row
/// is declared at compile time: a workspace's skill sessions contribute rows
/// whose labels come from its own `skill_sessions` env array (see
/// [`crate::skill_session`]). The static table below still fixes the order of
/// everything brain declares itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaletteRow {
    pub(crate) number: usize,
    pub(crate) label: String,
    pub(crate) action: PaletteAction,
    pub(crate) shortcut: Option<&'static str>,
}

pub(crate) struct PaletteCommand {
    pub(super) label: &'static str,
    pub(crate) action: PaletteAction,
    pub(super) scope: PaletteScope,
    /// Only consulted for `TaskSpecific` commands. When false, the
    /// command is hidden when the selected entry is a habit. (E.g. defer
    /// applies to tasks only; mark-complete works on either.)
    pub(super) works_on_habits: bool,
    /// Extra conditional-visibility gate, applied on top of `scope` /
    /// `works_on_habits`. Defaults to [`always`]. See [`VisibleWhen`].
    pub(super) is_visible: VisibleWhen,
}

// --- Visibility predicates (referenced from the const command table) ---

/// Always visible (subject only to the structural scope gate).
fn always(_: &PaletteState) -> bool {
    true
}

/// Visible only while the main brain panel is open.
fn if_brain_open(s: &PaletteState) -> bool {
    s.brain_open
}

/// Visible only while at least one skill-session tab is open — there is nothing
/// to switch back *from* otherwise.
fn if_skill_session_open(s: &PaletteState) -> bool {
    !s.open_skill_sessions.is_empty()
}

/// Visible only when the in-context entry has notes to expand/collapse.
fn if_has_notes(s: &PaletteState) -> bool {
    s.context_has_notes
}

/// Visible only when the in-context entry has at least one openable link.
fn if_has_links(s: &PaletteState) -> bool {
    s.context_links != LinkKind::None
}

fn if_assignment_create(s: &PaletteState) -> bool {
    s.assignment_mode.show_create_control
}

fn if_assignment_reassign(s: &PaletteState) -> bool {
    s.assignment_mode.show_reassign_control
}

fn if_assignment_filter(s: &PaletteState) -> bool {
    s.assignment_mode.show_filter
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PaletteScope {
    /// Available regardless of selection state.
    Global,
    /// Operates on the currently-selected task; hidden when no task is
    /// selected, and the *only* commands shown when the palette is opened
    /// in the task actions modal (Enter on a task).
    TaskSpecific,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PaletteAction {
    /// Ask the brain agent to collect a new task, preserving actor assignment
    /// as the default unless the user explicitly selects another member.
    AddTask,
    /// Open (or focus) the persistent brain panel, resuming the shell's
    /// most-recently-active session. The user types directly into it.
    SendBrainMessage,
    /// Close the brain panel and end its agent session. Only offered while
    /// a panel is open.
    CloseBrain,
    ToggleReceiver,
    ShowReceiverServerStatus,
    ShowReceiverServerLogs,
    ShowBrainLogs,
    ReturnToMainView,
    /// Like `SendBrainMessage`, but the entered text is prefixed with
    /// "This message is about <ID>:" so the brain agent knows which
    /// task / habit the user is asking about. Requires a selection.
    MessageBrainAboutTask,
    /// Completes the selected row natively, then reloads
    /// tasks.csv + habits.csv. Works for both tasks and habits.
    MarkTaskComplete,
    /// Spawn the brain panel with a prefilled "defer this task by N days"
    /// message. The brain agent (via the /todo skill) translates that
    /// into a `defer_task.py <id> +Nd` call. Tasks only — defer semantics
    /// for habits live elsewhere (`defer_habit.py`).
    DeferTask(u32),
    /// Spawn the brain panel with a "let's start this task" prompt that
    /// asks the agent to gather context and propose first steps + ways
    /// it can directly help. Tasks only — habits don't have "first
    /// steps" in the same sense.
    StartTask,
    /// Spawn the brain panel with a "remove this task" prompt. The brain
    /// agent (via `/todo remove`) decides between deletion vs.
    /// status="dropped" based on the row state. Tasks only — habits
    /// have their own removal flow.
    RemoveTask,
    /// Ask the brain agent to reassign the selected task or habit.
    ReassignTask,
    /// Open the native portable-member picker that filters the current view.
    ChooseAssigneeFilter,
    /// Open today's habits page in the browser, served by the bundled brain
    /// server already attached to the live TUI.
    /// Global.
    OpenHabitsInBrowser,
    /// Kick a best-effort background `brain sync` now. Global; no shortcut.
    SyncBrainNow,
    /// Report whether a sync is currently running. Global; no shortcut.
    ShowSyncStatus,
    /// Open today's agenda — same code path as the `Ctrl+A` shortcut.
    /// Routes through the `agenda` zsh function, which generates the
    /// PDF if needed and opens it. On failure (no markdown for today)
    /// surfaces the GenerateAgenda confirm modal. Global.
    OpenAgenda,
    /// Toggle the selected entry's notes between a single-line preview and
    /// the full markdown-rendered body. Task-specific; only offered when
    /// the entry actually has notes. Works on habits too.
    ToggleNotes,
    /// Open the selected entry's link(s) via `/usr/bin/open <url>`: its
    /// Linear issue and/or any URLs in its notes. Offered whenever the entry
    /// has ≥ 1 link (Linear or notes); a single link opens directly, several
    /// raise the picker. The label reflects which (see `label_for`).
    OpenLinks,
    /// Toggle the daily-triage startup nudge for the current session — the
    /// runtime counterpart to the portable `enable_daily_triage_check` config
    /// variable that seeds it at startup. Flips
    /// `App::skip_daily_triage_check` (process-scoped, not persisted config) so
    /// a long-running TUI can suppress or restore the alert across day
    /// rollovers. Global; label swaps Disable/Enable (see `label_for`).
    ToggleDailyTriageAlert,
    /// Focus the main brain session tab (`BrainTab::Main`) — the palette-driven
    /// counterpart to `Alt+1`, reliable on terminals where Alt+digit is not.
    /// Only offered while a skill-session tab is open (nothing to switch away
    /// from otherwise).
    ShowMainBrainSession,
    /// Start a skill session: a dedicated ephemeral tab seeded with that
    /// session's prompt, which closes itself when the run signals completion.
    /// One row per session the workspace offers (the builtin daily triage plus
    /// its `skill_sessions` env entries), each hidden while its own session is
    /// already running so it can't be started twice.
    RunSkillSession(SkillSessionKey),
    /// Focus a running skill session's tab — the palette-driven counterpart to
    /// its `Alt+<n>`. Only offered while that session is open.
    ShowSkillSession(SkillSessionKey),
}

/// Direct keystroke that bypasses the palette for a given action,
/// rendered as a dim `[…]` annotation next to the palette label.
/// Returns `None` when an action has no direct shortcut.
pub(crate) const fn shortcut_for(action: PaletteAction) -> Option<&'static str> {
    match action {
        PaletteAction::MarkTaskComplete => Some("^D"),
        PaletteAction::RemoveTask => Some("^⌫"),
        PaletteAction::MessageBrainAboutTask => Some("^⇧M"),
        PaletteAction::SendBrainMessage => Some("^M"),
        PaletteAction::CloseBrain => Some("^X"),
        PaletteAction::OpenHabitsInBrowser => Some("^H"),
        PaletteAction::OpenAgenda => Some("^A"),
        PaletteAction::ToggleNotes => Some("l"),
        PaletteAction::OpenLinks => Some("^O"),
        // No per-command hint: the tab switch is a cycle (`Alt+[` / `Alt+]`),
        // not a per-tab key, and these palette rows are themselves the reliable
        // switch (the direct `Alt+1` / `Alt+2` are terminal-unreliable).
        PaletteAction::ShowMainBrainSession
        | PaletteAction::RunSkillSession(_)
        | PaletteAction::ShowSkillSession(_)
        | PaletteAction::ToggleReceiver
        | PaletteAction::ShowReceiverServerStatus
        | PaletteAction::ShowReceiverServerLogs
        | PaletteAction::ShowBrainLogs
        | PaletteAction::ReturnToMainView
        | PaletteAction::StartTask
        | PaletteAction::DeferTask(_)
        | PaletteAction::SyncBrainNow
        | PaletteAction::ShowSyncStatus
        | PaletteAction::ToggleDailyTriageAlert
        | PaletteAction::AddTask
        | PaletteAction::ReassignTask
        | PaletteAction::ChooseAssigneeFilter => None,
    }
}

mod catalog;

pub(super) use catalog::PALETTE_COMMANDS;
