//! `PaletteState` behavior: the constructors for the global palette and the
//! task-actions modal, contextual label resolution, scope/filter derivation of
//! the visible rows, and cursor movement / text editing. The struct itself
//! lives in the `tui` root so every submodule can reach its fields.

use crate::tasks::task::AssignmentUiMode;
use crate::tui::*;

use super::command::{PALETTE_COMMANDS, PaletteRow, PaletteScope};

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
            receiver_enabled: false,
            runnable_skill_sessions: Vec::new(),
            open_skill_sessions: Vec::new(),
            logs_view: false,
            daily_triage_alert_disabled: false,
            assignment_mode: hidden_assignment_mode(),
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
            receiver_enabled: false,
            runnable_skill_sessions: Vec::new(),
            open_skill_sessions: Vec::new(),
            logs_view: false,
            daily_triage_alert_disabled: false,
            assignment_mode: hidden_assignment_mode(),
        }
    }

    pub(crate) const fn new_logs_view(receiver_enabled: bool) -> Self {
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
            receiver_enabled,
            runnable_skill_sessions: Vec::new(),
            open_skill_sessions: Vec::new(),
            logs_view: true,
            daily_triage_alert_disabled: false,
            assignment_mode: hidden_assignment_mode(),
        }
    }

    /// Seed the selected workspace's per-surface assignment visibility.
    #[must_use]
    pub(crate) const fn with_assignment_mode(mut self, mode: AssignmentUiMode) -> Self {
        self.assignment_mode = mode;
        self
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
        // The daily-triage toggle names the action that will happen next:
        // "Disable" while the alert is active, "Enable" while it's suppressed.
        if matches!(cmd.action, PaletteAction::ToggleDailyTriageAlert) {
            return if self.daily_triage_alert_disabled {
                "Enable daily triage alert".to_owned()
            } else {
                "Disable daily triage alert".to_owned()
            };
        }
        if matches!(cmd.action, PaletteAction::ToggleReceiver) {
            return if self.receiver_enabled {
                "Disable receiver".to_owned()
            } else {
                "Enable receiver".to_owned()
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
            PaletteAction::ReassignTask => format!("Reassign {id}"),
            PaletteAction::MessageBrainAboutTask => format!("Message brain about {id}"),
            PaletteAction::StartTask => format!("Start {id}"),
            // `OpenLinks` and `ToggleNotes` are resolved above; global
            // actions don't reach this branch (filtered above). Fall
            // through defensively.
            PaletteAction::OpenLinks
            | PaletteAction::AddTask
            | PaletteAction::ChooseAssigneeFilter
            | PaletteAction::SendBrainMessage
            | PaletteAction::CloseBrain
            | PaletteAction::OpenHabitsInBrowser
            | PaletteAction::SyncBrainNow
            | PaletteAction::ShowSyncStatus
            | PaletteAction::OpenAgenda
            | PaletteAction::ToggleNotes
            | PaletteAction::ToggleReceiver
            | PaletteAction::ShowReceiverServerStatus
            | PaletteAction::ShowReceiverServerLogs
            | PaletteAction::ShowBrainLogs
            | PaletteAction::ReturnToMainView
            | PaletteAction::ToggleDailyTriageAlert
            | PaletteAction::ShowMainBrainSession
            | PaletteAction::RunSkillSession(_)
            | PaletteAction::ShowSkillSession(_) => cmd.label.to_owned(),
        }
    }

    /// Every row the active scope permits, in canonical order and *before* the
    /// text filter, each carrying the stable 1-based number shown in the palette
    /// (so the digit a user types always points at the same row, mirroring the
    /// brain menu's numbered rows).
    ///
    /// The workspace's skill-session rows are spliced into the brain group: the
    /// sessions that can be *started* now sit right after **Message brain** (an
    /// always-present anchor, so their position doesn't move when a session
    /// opens), and each running session's tab switch follows **Show main brain
    /// session**. A running session contributes no start row, so the same session
    /// can never be launched twice.
    pub(crate) fn rows(&self) -> Vec<PaletteRow> {
        let mut rows: Vec<PaletteRow> = Vec::new();
        for command in PALETTE_COMMANDS
            .iter()
            .filter(|c| self.command_in_scope(c) && (c.is_visible)(self))
        {
            push_row(&mut rows, self.label_for(command), command.action);
            match command.action {
                PaletteAction::SendBrainMessage => {
                    for (key, label) in &self.runnable_skill_sessions {
                        push_row(&mut rows, label.clone(), PaletteAction::RunSkillSession(*key));
                    }
                }
                PaletteAction::ShowMainBrainSession => {
                    for (key, title) in &self.open_skill_sessions {
                        push_row(
                            &mut rows,
                            format!("Show {title} session"),
                            PaletteAction::ShowSkillSession(*key),
                        );
                    }
                }
                _ => {}
            }
        }
        rows
    }

    /// The structural scope gate: task-vs-global, the habit filter, the
    /// logs-view command whitelist, and the task-actions-modal restriction. The
    /// finer *conditional* gates (panel open, server running, a skill session
    /// open, notes/links present) live in each command's `is_visible` predicate
    /// and are applied on top of this by [`Self::rows`].
    fn command_in_scope(&self, c: &PaletteCommand) -> bool {
        if self.logs_view {
            // The logs palette shows only this fixed set of read-only /
            // navigation commands; each command's own `is_visible` still
            // applies.
            return matches!(
                c.action,
                PaletteAction::ToggleReceiver
                    | PaletteAction::ShowReceiverServerStatus
                    | PaletteAction::ShowSyncStatus
                    | PaletteAction::ShowReceiverServerLogs
                    | PaletteAction::ShowBrainLogs
                    | PaletteAction::ReturnToMainView
            );
        }
        match c.scope {
            PaletteScope::TaskSpecific => {
                self.task_in_context() && (!self.context_is_habit || c.works_on_habits)
            }
            PaletteScope::Global => !self.task_actions_modal,
        }
    }

    /// Rows matching the current filter (case-insensitive substring over the
    /// numbered, displayed label `"N. label"`, so users can narrow by row
    /// number, label word, or task ID) AND the active scope.
    pub(crate) fn visible(&self) -> Vec<PaletteRow> {
        let q = self.filter.to_lowercase();
        self.rows()
            .into_iter()
            .filter(|row| {
                q.is_empty() || {
                    format!("{}. {}", row.number, row.label)
                        .to_lowercase()
                        .contains(&q)
                }
            })
            .collect()
    }

    /// The rendered rows: each visible row's numbered label (`"N. …"`) paired
    /// with its direct-key shortcut hint, if any.
    pub(crate) fn numbered_entries(&self) -> Vec<(String, Option<&'static str>)> {
        self.visible()
            .iter()
            .map(|row| (format!("{}. {}", row.number, row.label), row.shortcut))
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
        self.visible().get(self.selected).map(|row| row.action)
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

/// Append one row, numbering it by its position. The number is what the palette
/// shows and what a typed digit selects, so it must stay 1-based and gapless.
fn push_row(rows: &mut Vec<PaletteRow>, label: String, action: PaletteAction) {
    rows.push(PaletteRow {
        number: rows.len() + 1,
        label,
        action,
        shortcut: shortcut_for(action),
    });
}

const fn hidden_assignment_mode() -> AssignmentUiMode {
    AssignmentUiMode {
        show_in_detail: false,
        show_create_control: false,
        show_reassign_control: false,
        show_filter: false,
    }
}
