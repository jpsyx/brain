//! `PaletteState` behavior: the constructors for the global palette and the
//! task-actions modal, contextual label resolution, scope/filter derivation of
//! the visible rows, and cursor movement / text editing. The struct itself
//! lives in the `tui` root so every submodule can reach its fields.

use crate::tui::*;

use super::command::{PALETTE_COMMANDS, PaletteScope};

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
        _logs_available: bool,
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
            receiver_server_running: false,
            logs_view: false,
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
            receiver_server_running: false,
            logs_view: false,
        }
    }

    pub(crate) const fn new_logs_view(receiver_server_running: bool) -> Self {
        Self {
            filter: String::new(),
            selected: 0,
            task_id: None,
            task_label: None,
            context_is_habit: false,
            context_has_notes: false,
            context_notes_expanded: false,
            context_links: LinkKind::None,
            task_actions_modal: false,
            brain_open: false,
            receiver_server_running,
            logs_view: true,
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
            | PaletteAction::SyncBrainNow
            | PaletteAction::ShowSyncStatus
            | PaletteAction::OpenAgenda
            | PaletteAction::ToggleNotes
            | PaletteAction::StartReceiverServer
            | PaletteAction::StopReceiverServer
            | PaletteAction::RestartReceiverServer
            | PaletteAction::ShowReceiverServerStatus
            | PaletteAction::ShowReceiverServerLogs
            | PaletteAction::ShowBrainLogs
            | PaletteAction::ReturnToMainView => cmd.label.to_owned(),
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
                    if self.logs_view {
                        return matches!(
                            c.action,
                            PaletteAction::ShowReceiverServerStatus
                                | PaletteAction::ShowSyncStatus
                                | PaletteAction::ShowReceiverServerLogs
                                | PaletteAction::ShowBrainLogs
                                | PaletteAction::ReturnToMainView
                        ) && (!matches!(c.action, PaletteAction::ShowReceiverServerLogs)
                            || self.receiver_server_running);
                    }
                    !self.task_actions_modal
                        // "Close brain" only makes sense while a panel is open.
                        && (!matches!(c.action, PaletteAction::CloseBrain) || self.brain_open)
                        && match c.action {
                            PaletteAction::StartReceiverServer => !self.receiver_server_running,
                            PaletteAction::StopReceiverServer
                            | PaletteAction::RestartReceiverServer
                            | PaletteAction::ShowReceiverServerLogs => self.receiver_server_running,
                            _ => true,
                        }
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
