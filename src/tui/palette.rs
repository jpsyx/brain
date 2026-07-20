//! Command-palette commands, the action enum, and `PaletteState` behavior.
//!
//! The `PaletteState` struct itself lives in the crate root; this file owns
//! the command table and the impl.

use super::*;


pub(crate) struct PaletteCommand {
    label: &'static str,
    pub(crate) action: PaletteAction,
    scope: PaletteScope,
    /// Only consulted for `TaskSpecific` commands. When false, the
    /// command is hidden when the selected entry is a habit. (E.g. defer
    /// applies to tasks only; mark-complete works on either.)
    works_on_habits: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteScope {
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
    /// Close the brain panel and end its claude session. Only offered while
    /// a panel is open.
    CloseBrain,
    /// Like `SendBrainMessage`, but the entered text is prefixed with
    /// "This message is about <ID>:" so the brain agent knows which
    /// task / habit the user is asking about. Requires a selection.
    MessageBrainAboutTask,
    /// Runs `mark_done.py <selected-id>` synchronously, then reloads
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
    /// Run the `habits` zsh function — opens today's habits page in the
    /// browser, reusing or starting the local server in
    /// `~/scripts/rc/habits/`. Global.
    OpenHabitsInBrowser,
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
        PaletteAction::StartTask | PaletteAction::DeferTask(_) => None,
    }
}

// Order here is the order shown in both palettes (`visible()` preserves
// it). The task actions modal simply filters out the `Global` entries, so the
// task-scoped commands keep this same relative order in both views:
// start → complete → message-about → message-global → notes → remove →
// defer group → other globals.
const PALETTE_COMMANDS: &[PaletteCommand] = &[
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
        label: "Open today's agenda",
        action: PaletteAction::OpenAgenda,
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

impl PaletteState {
    /// Open the global command palette (global + any task-specific commands the
    /// context permits).
    pub(crate) const fn new(
        task_id: Option<String>,
        context_is_habit: bool,
        context_has_notes: bool,
        context_notes_expanded: bool,
        context_links: LinkKind,
        brain_open: bool,
    ) -> Self {
        Self {
            filter: String::new(),
            selected: 0,
            task_id,
            task_label: None,
            context_is_habit,
            context_has_notes,
            context_notes_expanded,
            context_links,
            task_actions_modal: false,
            brain_open,
        }
    }

    /// Open the task actions modal. Caller must guarantee a task is selected
    /// (the ID is required for the modal title; the label is shown as a
    /// dim subtitle).
    pub(crate) const fn new_task_actions(
        task_id: String,
        task_label: String,
        context_is_habit: bool,
        context_has_notes: bool,
        context_notes_expanded: bool,
        context_links: LinkKind,
    ) -> Self {
        Self {
            filter: String::new(),
            selected: 0,
            task_id: Some(task_id),
            task_label: Some(task_label),
            context_is_habit,
            context_has_notes,
            context_notes_expanded,
            context_links,
            task_actions_modal: true,
            // The task actions modal only shows task-scoped commands, so the
            // global "Close brain" never appears here regardless.
            brain_open: false,
        }
    }

    pub(crate) const fn task_in_context(&self) -> bool {
        self.task_id.is_some()
    }

    /// Resolve the display label for a command in the current context.
    /// The task actions modal keeps the static `cmd.label` (the title already
    /// names the task). In the global command palette, task-specific commands
    /// interpolate the task ID so users can see what they'd be
    /// operating on at a glance.
    pub(crate) fn label_for(&self, cmd: &PaletteCommand) -> String {
        // The notes toggle's label tracks current expansion state in both
        // the task actions modal and the global command palette. In the global command palette it also
        // names the entry (e.g. "Expand T123 notes"), matching the
        // task-ID convention of the other task-specific commands.
        if matches!(cmd.action, PaletteAction::ToggleNotes) {
            let verb = if self.context_notes_expanded {
                "Collapse"
            } else {
                "Expand"
            };
            return match (self.task_actions_modal, &self.task_id) {
                (false, Some(id)) => format!("{verb} {id} notes"),
                _ => format!("{verb} notes"),
            };
        }
        // The "open link" command's wording depends on what it'll open:
        // a lone Linear issue, a lone notes URL, or several links (→ picker).
        // The global palette names the task; the actions modal doesn't (its
        // title already does).
        if matches!(cmd.action, PaletteAction::OpenLinks) {
            let named = (!self.task_actions_modal)
                .then_some(self.task_id.as_deref())
                .flatten();
            return match (named, self.context_links) {
                (Some(id), LinkKind::SingleLinear) => format!("Open {id} Linear link"),
                (Some(id), LinkKind::SingleNotes) => format!("Open link from {id}'s note"),
                (Some(id), LinkKind::Multiple) => format!("Open link attached to {id}"),
                (None, LinkKind::SingleLinear) => "Open Linear link".to_owned(),
                (None, LinkKind::SingleNotes) => "Open link from note".to_owned(),
                (None, LinkKind::Multiple) => "Open attached link".to_owned(),
                (_, LinkKind::None) => cmd.label.to_owned(),
            };
        }
        if self.task_actions_modal || cmd.scope != PaletteScope::TaskSpecific {
            return cmd.label.to_owned();
        }
        let Some(id) = &self.task_id else {
            return cmd.label.to_owned();
        };
        match cmd.action {
            PaletteAction::MarkTaskComplete => format!("Mark {id} as complete"),
            PaletteAction::DeferTask(days) => format!("Defer {id} +{days}d"),
            PaletteAction::RemoveTask => format!("Remove task {id}"),
            PaletteAction::MessageBrainAboutTask => format!("Message brain about {id}"),
            PaletteAction::StartTask => format!("Start {id}"),
            // `OpenLinks` and `ToggleNotes` are resolved above; global
            // actions don't reach this branch (filtered above). Fall
            // through defensively.
            PaletteAction::OpenLinks
            | PaletteAction::SendBrainMessage
            | PaletteAction::CloseBrain
            | PaletteAction::OpenHabitsInBrowser
            | PaletteAction::OpenAgenda
            | PaletteAction::ToggleNotes => cmd.label.to_owned(),
        }
    }

    /// Commands the active scope permits, in canonical order and *before*
    /// the text filter. These carry the stable 1-based numbers shown in the
    /// palette (so the digit a user types always points at the same command,
    /// mirroring the brain menu's numbered rows).
    pub(crate) fn scoped(&self) -> Vec<&'static PaletteCommand> {
        PALETTE_COMMANDS
            .iter()
            .filter(|c| match c.scope {
                PaletteScope::TaskSpecific => {
                    self.task_in_context()
                        && (!self.context_is_habit || c.works_on_habits)
                        // The notes toggle only makes sense when there are
                        // notes to expand.
                        && (!matches!(c.action, PaletteAction::ToggleNotes)
                            || self.context_has_notes)
                        // The "open link" command only makes sense when the
                        // entry has at least one openable link (Linear or
                        // a notes URL).
                        && (!matches!(c.action, PaletteAction::OpenLinks)
                            || self.context_links != LinkKind::None)
                }
                PaletteScope::Global => {
                    !self.task_actions_modal
                        // "Close brain" only makes sense while a panel is open.
                        && (!matches!(c.action, PaletteAction::CloseBrain) || self.brain_open)
                }
            })
            .collect()
    }

    /// The stable 1-based number shown next to `cmd`: its position in the
    /// scope-visible list. `0` if the command isn't in scope (shouldn't
    /// happen for a rendered row).
    pub(crate) fn number_for(&self, cmd: &PaletteCommand) -> usize {
        self.scoped()
            .iter()
            .position(|c| c.action == cmd.action)
            .map_or(0, |i| i + 1)
    }

    /// Commands matching the current filter (case-insensitive substring over
    /// the numbered, displayed label `"N. label"`, so users can narrow by
    /// row number, label word, or task ID) AND the active scope.
    pub(crate) fn visible(&self) -> Vec<&'static PaletteCommand> {
        let q = self.filter.to_lowercase();
        self.scoped()
            .into_iter()
            .filter(|c| {
                q.is_empty() || {
                    format!("{}. {}", self.number_for(c), self.label_for(c))
                        .to_lowercase()
                        .contains(&q)
                }
            })
            .collect()
    }

    /// The rendered rows: each visible command's numbered label (`"N. …"`)
    /// paired with its direct-key shortcut hint, if any.
    pub(crate) fn numbered_entries(&self) -> Vec<(String, Option<&'static str>)> {
        self.visible()
            .iter()
            .map(|c| {
                (
                    format!("{}. {}", self.number_for(c), self.label_for(c)),
                    shortcut_for(c.action),
                )
            })
            .collect()
    }

    pub(crate) fn title(&self) -> String {
        if self.task_actions_modal {
            if let Some(id) = &self.task_id {
                return format!("Task {id} actions");
            }
            return "Task actions".to_owned();
        }
        "Command palette".to_owned()
    }

    pub(crate) fn selected_action(&self) -> Option<PaletteAction> {
        self.visible().get(self.selected).map(|c| c.action)
    }

    pub(crate) fn move_down(&mut self) {
        let n = self.visible().len();
        if n > 0 {
            self.selected = (self.selected + 1) % n;
        }
    }

    pub(crate) fn move_up(&mut self) {
        let n = self.visible().len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }

    pub(crate) fn append(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
    }

    pub(crate) fn pop(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}
