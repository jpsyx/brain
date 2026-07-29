//! The command-palette model: the action enum, the per-command scope/flags,
//! the direct-key shortcut map, and the ordered command table.

pub(crate) struct PaletteCommand {
    pub(super) label: &'static str,
    pub(crate) action: PaletteAction,
    pub(super) scope: PaletteScope,
    /// Only consulted for `TaskSpecific` commands. When false, the
    /// command is hidden when the selected entry is a habit. (E.g. defer
    /// applies to tasks only; mark-complete works on either.)
    pub(super) works_on_habits: bool,
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
    /// Open (or focus) the persistent brain panel, resuming the shell's
    /// most-recently-active session. The user types directly into it.
    SendBrainMessage,
    /// Close the brain panel and end its agent session. Only offered while
    /// a panel is open.
    CloseBrain,
    StartReceiverServer,
    StopReceiverServer,
    RestartReceiverServer,
    ShowReceiverServerLogs,
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
    /// Open today's habits page in the browser, served by the bundled brain
    /// server (started on demand via `server::lifecycle::ensure_running`).
    /// Global.
    OpenHabitsInBrowser,
    /// Kick a best-effort background `brain sync` now. Global; no shortcut.
    SyncBrainNow,
    /// Open today's agenda — same code path as the `Ctrl+A` shortcut.
    /// Routes through the `agenda` zsh function, which generates the
    /// PDF if needed and opens it. On failure (no markdown for today)
    /// surfaces the GenerateAgenda confirm modal. Global.
    OpenAgenda,
    /// Ask whether to reveal this run's verbose log file in Finder and open
    /// the file. Global; only present when this TUI has a verbose log file.
    ShowLogs,
    /// Toggle the selected entry's notes between a single-line preview and
    /// the full markdown-rendered body. Task-specific; only offered when
    /// the entry actually has notes. Works on habits too.
    ToggleNotes,
    /// Open the selected entry's link(s) via `/usr/bin/open <url>`: its
    /// Linear issue and/or any URLs in its notes. Offered whenever the entry
    /// has ≥ 1 link (Linear or notes); a single link opens directly, several
    /// raise the picker. The label reflects which (see `label_for`).
    OpenLinks,
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
        PaletteAction::ShowLogs
        | PaletteAction::StartReceiverServer
        | PaletteAction::StopReceiverServer
        | PaletteAction::RestartReceiverServer
        | PaletteAction::ShowReceiverServerLogs
        | PaletteAction::StartTask
        | PaletteAction::DeferTask(_)
        | PaletteAction::SyncBrainNow => None,
    }
}

// Order here is the order shown in both palettes (`visible()` preserves
// it). The task actions modal simply filters out the `Global` entries, so the
// task-scoped commands keep this same relative order in both views:
// start → complete → message-about → message-global → notes → remove →
// defer group → other globals.
pub(super) const PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        label: "Start this task",
        action: PaletteAction::StartTask,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Mark as complete",
        action: PaletteAction::MarkTaskComplete,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: true,
    },
    PaletteCommand {
        label: "Message brain about this task",
        action: PaletteAction::MessageBrainAboutTask,
        scope: PaletteScope::TaskSpecific,
        // Asking the brain agent about a habit reads fine — the
        // context prefix just names the H-ID instead of a T-ID.
        works_on_habits: true,
    },
    PaletteCommand {
        label: "Message brain",
        action: PaletteAction::SendBrainMessage,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Close brain",
        action: PaletteAction::CloseBrain,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Start receiver server",
        action: PaletteAction::StartReceiverServer,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Stop receiver server",
        action: PaletteAction::StopReceiverServer,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Restart receiver server",
        action: PaletteAction::RestartReceiverServer,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Show receiver logs",
        action: PaletteAction::ShowReceiverServerLogs,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        // Label is overridden at render time (Expand vs Collapse) by
        // `label_for`; this static is the fallback.
        label: "Expand notes",
        action: PaletteAction::ToggleNotes,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: true,
    },
    PaletteCommand {
        label: "Remove this task",
        action: PaletteAction::RemoveTask,
        scope: PaletteScope::TaskSpecific,
        // Habit removal goes through a different /todo path; keep this
        // tasks only to avoid sending the wrong instruction.
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Defer +1d",
        action: PaletteAction::DeferTask(1),
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Defer +7d",
        action: PaletteAction::DeferTask(7),
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Defer +14d",
        action: PaletteAction::DeferTask(14),
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Open habits in browser",
        action: PaletteAction::OpenHabitsInBrowser,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Sync brain now",
        action: PaletteAction::SyncBrainNow,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Open today's agenda",
        action: PaletteAction::OpenAgenda,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        label: "Show logs",
        action: PaletteAction::ShowLogs,
        scope: PaletteScope::Global,
        works_on_habits: false,
    },
    PaletteCommand {
        // Static fallback label; `label_for` overrides it per link kind.
        label: "Open link",
        action: PaletteAction::OpenLinks,
        scope: PaletteScope::TaskSpecific,
        // Habits never link to Linear, but they can carry notes URLs — the
        // link-kind gate below hides the command unless there's ≥ 1 link.
        works_on_habits: true,
    },
];
