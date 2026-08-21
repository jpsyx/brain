use super::{
    PaletteAction, PaletteCommand, PaletteScope, always, if_assignment_create,
    if_assignment_filter, if_assignment_reassign, if_brain_open, if_has_links, if_has_notes,
    if_skill_session_open,
};

// Order here is the order shown in both palettes (`visible()` preserves
// it). The task actions modal simply filters out the `Global` entries, so the
// task-scoped commands keep this same relative order in both views:
// start → complete → message-about → message-global → notes → remove →
// defer group → other globals.
pub(in crate::tui::palette) const PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        label: "Add task",
        action: PaletteAction::AddTask,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: if_assignment_create,
    },
    PaletteCommand {
        label: "Start this task",
        action: PaletteAction::StartTask,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Mark as complete",
        action: PaletteAction::MarkTaskComplete,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: true,
        is_visible: always,
    },
    PaletteCommand {
        label: "Message brain about this task",
        action: PaletteAction::MessageBrainAboutTask,
        scope: PaletteScope::TaskSpecific,
        // Asking the brain agent about a habit reads fine — the
        // context prefix just names the H-ID instead of a T-ID.
        works_on_habits: true,
        is_visible: always,
    },
    PaletteCommand {
        label: "Message brain",
        action: PaletteAction::SendBrainMessage,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Close brain",
        action: PaletteAction::CloseBrain,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: if_brain_open,
    },
    // The workspace's skill-session rows (start / focus) are spliced in around
    // this row at build time — see `PaletteState::rows`.
    PaletteCommand {
        label: "Show main brain session",
        action: PaletteAction::ShowMainBrainSession,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: if_skill_session_open,
    },
    PaletteCommand {
        // Label is overridden at render time from persistent workspace intent.
        label: "Enable receiver",
        action: PaletteAction::ToggleReceiver,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Show receiver server status",
        action: PaletteAction::ShowReceiverServerStatus,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Show receiver logs",
        action: PaletteAction::ShowReceiverServerLogs,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        // Label is overridden at render time (Expand vs Collapse) by
        // `label_for`; this static is the fallback.
        label: "Expand notes",
        action: PaletteAction::ToggleNotes,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: true,
        is_visible: if_has_notes,
    },
    PaletteCommand {
        label: "Remove this task",
        action: PaletteAction::RemoveTask,
        scope: PaletteScope::TaskSpecific,
        // Habit removal goes through a different /todo path; keep this
        // tasks only to avoid sending the wrong instruction.
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Reassign this task",
        action: PaletteAction::ReassignTask,
        scope: PaletteScope::TaskSpecific,
        works_on_habits: true,
        is_visible: if_assignment_reassign,
    },
    PaletteCommand {
        label: "Filter by assignee",
        action: PaletteAction::ChooseAssigneeFilter,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: if_assignment_filter,
    },
    PaletteCommand {
        label: "Defer +1d",
        action: PaletteAction::DeferTask(1),
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Defer +7d",
        action: PaletteAction::DeferTask(7),
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Defer +14d",
        action: PaletteAction::DeferTask(14),
        scope: PaletteScope::TaskSpecific,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Open habits in browser",
        action: PaletteAction::OpenHabitsInBrowser,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Sync brain now",
        action: PaletteAction::SyncBrainNow,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Show sync status",
        action: PaletteAction::ShowSyncStatus,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Open today's agenda",
        action: PaletteAction::OpenAgenda,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Show brain logs",
        action: PaletteAction::ShowBrainLogs,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        // Label is overridden at render time (Disable vs Enable) by
        // `label_for` from the seeded `daily_triage_alert_disabled`; this
        // static is the fallback.
        label: "Disable daily triage alert",
        action: PaletteAction::ToggleDailyTriageAlert,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        label: "Return to main view",
        action: PaletteAction::ReturnToMainView,
        scope: PaletteScope::Global,
        works_on_habits: false,
        is_visible: always,
    },
    PaletteCommand {
        // Static fallback label; `label_for` overrides it per link kind.
        label: "Open link",
        action: PaletteAction::OpenLinks,
        scope: PaletteScope::TaskSpecific,
        // Habits never link to Linear, but they can carry notes URLs — the
        // link-kind gate below hides the command unless there's ≥ 1 link.
        works_on_habits: true,
        is_visible: if_has_links,
    },
];
