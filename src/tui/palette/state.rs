//! `TaskPalette` behavior: contextual catalog construction around the shared
//! command-palette state used by both TUI surfaces.

use crate::tasks::task::AssignmentUiMode;
use crate::tui::action::GlobalAction;
use crate::tui::links::LinkKind;
use crate::tui::modal_state::TaskPalette;

use super::command::{PALETTE_COMMANDS, PaletteCommand, PaletteScope, TaskAction, shortcut_for};
use super::model::{CommandPalette, PaletteControls, PaletteRow, PaletteStep};

impl TaskPalette {
    /// Open the global command palette (global + any task-specific commands the
    /// context permits).
    pub(crate) fn new(
        task_id: Option<String>,
        context_is_habit: bool,
        context_has_notes: bool,
        context_notes_expanded: bool,
        context_links: LinkKind,
        brain_open: bool,
        _logs_available: bool,
    ) -> Self {
        Self {
            palette: empty_palette(),
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
        .rebuild_palette()
    }

    /// Open the task actions modal. Caller must guarantee a task is selected
    /// (the ID is required for the modal title; the label is shown as a
    /// dim subtitle).
    pub(crate) fn new_task_actions(
        task_id: String,
        task_label: String,
        context_is_habit: bool,
        context_has_notes: bool,
        context_notes_expanded: bool,
        context_links: LinkKind,
    ) -> Self {
        Self {
            palette: empty_palette(),
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
        .rebuild_palette()
    }

    pub(crate) fn new_logs_view(receiver_enabled: bool) -> Self {
        Self {
            palette: empty_palette(),
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
        .rebuild_palette()
    }

    /// Seed the selected workspace's per-surface assignment visibility.
    #[must_use]
    pub(crate) fn with_assignment_mode(mut self, mode: AssignmentUiMode) -> Self {
        self.assignment_mode = mode;
        self.rebuild_palette()
    }

    #[must_use]
    pub(crate) fn with_runtime_context(
        mut self,
        receiver_enabled: bool,
        daily_triage_alert_disabled: bool,
        runnable_skill_sessions: Vec<(crate::skill_session::SkillSessionKey, String)>,
        open_skill_sessions: Vec<(crate::skill_session::SkillSessionKey, String)>,
    ) -> Self {
        self.receiver_enabled = receiver_enabled;
        self.daily_triage_alert_disabled = daily_triage_alert_disabled;
        self.runnable_skill_sessions = runnable_skill_sessions;
        self.open_skill_sessions = open_skill_sessions;
        self.rebuild_palette()
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
        if matches!(cmd.action, TaskAction::ToggleNotes) {
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
        if matches!(
            cmd.action,
            TaskAction::Global(GlobalAction::ToggleDailyTriageAlert)
        ) {
            return if self.daily_triage_alert_disabled {
                "Enable daily triage alert".to_owned()
            } else {
                "Disable daily triage alert".to_owned()
            };
        }
        if matches!(cmd.action, TaskAction::Global(GlobalAction::ToggleReceiver)) {
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
        if matches!(cmd.action, TaskAction::OpenLinks) {
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
            TaskAction::MarkTaskComplete => format!("Mark {id} as complete"),
            TaskAction::DeferTask(days) => format!("Defer {id} +{days}d"),
            TaskAction::RemoveTask => format!("Remove task {id}"),
            TaskAction::ReassignTask => format!("Reassign {id}"),
            TaskAction::MessageBrainAboutTask => format!("Message brain about {id}"),
            TaskAction::StartTask => format!("Start {id}"),
            // `OpenLinks` and `ToggleNotes` are resolved above; global
            // actions don't reach this branch (filtered above). Fall
            // through defensively.
            TaskAction::OpenLinks
            | TaskAction::AddTask
            | TaskAction::ChooseAssigneeFilter
            | TaskAction::ToggleNotes
            | TaskAction::Global(_) => cmd.label.to_owned(),
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
    fn catalog_rows(&self) -> Vec<PaletteRow<TaskAction>> {
        let mut rows: Vec<PaletteRow<TaskAction>> = Vec::new();
        for command in PALETTE_COMMANDS
            .iter()
            .filter(|c| self.command_in_scope(c) && (c.is_visible)(self))
        {
            push_row(&mut rows, self.label_for(command), command.action);
            match command.action {
                TaskAction::Global(GlobalAction::MessageBrain) => {
                    for (key, label) in &self.runnable_skill_sessions {
                        push_row(
                            &mut rows,
                            label.clone(),
                            TaskAction::Global(GlobalAction::RunSkillSession(*key)),
                        );
                    }
                }
                TaskAction::Global(GlobalAction::ShowMainBrainSession) => {
                    for (key, title) in &self.open_skill_sessions {
                        push_row(
                            &mut rows,
                            format!("Show {title} session"),
                            TaskAction::Global(GlobalAction::ShowSkillSession(*key)),
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
    /// and are applied on top of this while the shared row state is built.
    fn command_in_scope(&self, c: &PaletteCommand) -> bool {
        if self.logs_view {
            // The logs palette shows only this fixed set of read-only /
            // navigation commands; each command's own `is_visible` still
            // applies.
            return matches!(
                c.action,
                TaskAction::Global(
                    GlobalAction::ToggleReceiver
                        | GlobalAction::ShowReceiverServerStatus
                        | GlobalAction::ShowSyncStatus
                        | GlobalAction::ShowReceiverServerLogs
                        | GlobalAction::ShowBrainLogs
                        | GlobalAction::ShowTasks
                )
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
    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[PaletteRow<TaskAction>] {
        self.palette.rows()
    }

    pub(crate) fn visible(&self) -> Vec<&PaletteRow<TaskAction>> {
        self.palette.visible()
    }

    /// The rendered rows: each visible row's numbered label (`"N. …"`) paired
    /// with its direct-key shortcut hint, if any.
    pub(crate) fn numbered_entries(&self) -> Vec<(String, Option<&'static str>)> {
        self.palette.numbered_entries()
    }

    fn catalog_title(&self) -> String {
        if self.task_actions_modal {
            if let Some(id) = &self.task_id {
                return format!("Task {id} actions");
            }
            return "Task actions".to_owned();
        }
        "Command palette".to_owned()
    }

    pub(crate) fn title(&self) -> &str {
        self.palette.title()
    }

    pub(crate) fn subtitle(&self) -> Option<&str> {
        self.palette.subtitle()
    }

    pub(crate) const fn task_actions_modal(&self) -> bool {
        self.task_actions_modal
    }

    pub(crate) fn selected(&self) -> usize {
        self.palette.selected()
    }

    pub(crate) fn query(&self) -> &str {
        self.palette.query()
    }

    pub(crate) fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> PaletteStep<TaskAction> {
        self.palette.handle_key(key)
    }

    fn rebuild_palette(mut self) -> Self {
        let title = self.catalog_title();
        let subtitle = self
            .task_actions_modal
            .then(|| self.task_label.clone())
            .flatten();
        self.palette =
            CommandPalette::new(title, subtitle, self.catalog_rows(), PaletteControls::TASKS);
        self
    }
}

/// Append one row, numbering it by its position. The number is what the palette
/// shows and what a typed digit selects, so it must stay 1-based and gapless.
fn push_row(rows: &mut Vec<PaletteRow<TaskAction>>, label: String, action: TaskAction) {
    let mut row = PaletteRow::new(label, action, shortcut_for(action));
    row.number = rows.len() + 1;
    rows.push(row);
}

const fn hidden_assignment_mode() -> AssignmentUiMode {
    AssignmentUiMode {
        show_in_detail: false,
        show_create_control: false,
        show_reassign_control: false,
        show_filter: false,
    }
}

fn empty_palette() -> CommandPalette<TaskAction> {
    CommandPalette::new("Command palette", None, Vec::new(), PaletteControls::TASKS)
}
