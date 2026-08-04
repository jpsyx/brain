//! Tests for PaletteState: notes toggle, open-link gating/labels, action
//! order, and numbered rows.

use crate::tui::*;

// --- PaletteState: notes toggle ---

fn has_toggle(state: &PaletteState) -> bool {
    state
        .visible()
        .iter()
        .any(|c| matches!(c.action, PaletteAction::ToggleNotes))
}

fn toggle_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::ToggleNotes))
        .map(|c| state.label_for(c))
}

#[test]
fn notes_toggle_hidden_when_task_has_no_notes() {
    let state = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::None,
    );
    assert!(!has_toggle(&state));
}

#[test]
fn notes_toggle_shown_and_reads_expand_when_collapsed() {
    let state = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        true,
        false,
        LinkKind::None,
    );
    assert!(has_toggle(&state));
    assert_eq!(toggle_label(&state).as_deref(), Some("Expand notes"));
}

#[test]
fn notes_toggle_reads_collapse_when_expanded() {
    let state = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        true,
        true,
        LinkKind::None,
    );
    assert_eq!(toggle_label(&state).as_deref(), Some("Collapse notes"));
}

#[test]
fn notes_toggle_available_for_habits_with_notes() {
    // Habits can carry notes too; the toggle is `works_on_habits`.
    let state = PaletteState::new_task_actions(
        "H1".into(),
        "habit".into(),
        true,
        true,
        false,
        LinkKind::None,
    );
    assert!(has_toggle(&state));
}

#[test]
fn notes_toggle_in_global_palette_names_the_task() {
    // In the global command palette the toggle follows the task-ID convention
    // of the other task-specific commands ("Expand T123 notes").
    let state = PaletteState::new(
        Some("T123".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    assert_eq!(toggle_label(&state).as_deref(), Some("Expand T123 notes"));
}

#[test]
fn notes_toggle_in_global_palette_reads_collapse_when_expanded() {
    let state = PaletteState::new(
        Some("T123".into()),
        false,
        true,
        true,
        LinkKind::None,
        false,
        false,
    );
    assert_eq!(toggle_label(&state).as_deref(), Some("Collapse T123 notes"));
}

// --- PaletteState: "open link" gating + per-kind label ---

fn has_open_links(state: &PaletteState) -> bool {
    state
        .visible()
        .iter()
        .any(|c| matches!(c.action, PaletteAction::OpenLinks))
}

fn open_links_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::OpenLinks))
        .map(|c| state.label_for(c))
}

#[test]
fn open_links_hidden_when_task_has_no_links() {
    let state = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::None,
    );
    assert!(!has_open_links(&state));
}

#[test]
fn open_links_single_linear_label() {
    // Actions modal (no id in the label) and global palette (named).
    let actions = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::SingleLinear,
    );
    assert!(has_open_links(&actions));
    assert_eq!(
        open_links_label(&actions).as_deref(),
        Some("Open Linear link")
    );

    let global = PaletteState::new(
        Some("T123".into()),
        false,
        false,
        false,
        LinkKind::SingleLinear,
        false,
        false,
    );
    assert_eq!(
        open_links_label(&global).as_deref(),
        Some("Open T123 Linear link")
    );
}

#[test]
fn open_links_single_notes_label() {
    let actions = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::SingleNotes,
    );
    assert!(has_open_links(&actions));
    assert_eq!(
        open_links_label(&actions).as_deref(),
        Some("Open link from note")
    );

    let global = PaletteState::new(
        Some("T90".into()),
        false,
        false,
        false,
        LinkKind::SingleNotes,
        false,
        false,
    );
    assert_eq!(
        open_links_label(&global).as_deref(),
        Some("Open link from T90's note")
    );
}

#[test]
fn open_links_multiple_label() {
    let actions = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        false,
        false,
        LinkKind::Multiple,
    );
    assert!(has_open_links(&actions));
    assert_eq!(
        open_links_label(&actions).as_deref(),
        Some("Open attached link")
    );

    let global = PaletteState::new(
        Some("T123".into()),
        false,
        false,
        false,
        LinkKind::Multiple,
        false,
        false,
    );
    assert_eq!(
        open_links_label(&global).as_deref(),
        Some("Open link attached to T123")
    );
}

#[test]
fn open_links_advertises_its_ctrl_o_shortcut() {
    // The `[^O]` hint renders next to the label in both modals, mirroring
    // the other directly-bound actions (^D, ^N, …).
    assert_eq!(shortcut_for(PaletteAction::OpenLinks), Some("^O"));
}

#[test]
fn assignment_palette_controls_are_visible_only_for_shared_workspaces() {
    let personal = PaletteState::new(
        Some("T1".into()),
        false,
        false,
        false,
        LinkKind::None,
        false,
        false,
    );
    let shared = PaletteState::new(
        Some("T1".into()),
        false,
        false,
        false,
        LinkKind::None,
        false,
        false,
    )
    .with_assignment_mode(crate::tasks::task::AssignmentUiMode {
        show_in_detail: true,
        show_create_control: true,
        show_reassign_control: true,
        show_filter: true,
    });

    for action in [
        PaletteAction::AddTask,
        PaletteAction::ReassignTask,
        PaletteAction::ChooseAssigneeFilter,
    ] {
        assert!(!action_order(&personal).contains(&action));
        assert!(action_order(&shared).contains(&action));
        assert_eq!(shortcut_for(action), None);
    }
}

#[test]
fn assignment_palette_uses_each_surface_visibility_flag_independently() {
    let asymmetric = PaletteState::new(
        Some("T1".into()),
        false,
        false,
        false,
        LinkKind::None,
        false,
        false,
    )
    .with_assignment_mode(crate::tasks::task::AssignmentUiMode {
        show_in_detail: false,
        show_create_control: true,
        show_reassign_control: false,
        show_filter: true,
    });
    let actions = action_order(&asymmetric);

    assert!(actions.contains(&PaletteAction::AddTask));
    assert!(!actions.contains(&PaletteAction::ReassignTask));
    assert!(actions.contains(&PaletteAction::ChooseAssigneeFilter));
}

#[test]
fn brain_logs_are_always_available() {
    let without_logs = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert!(
        action_order(&without_logs).contains(&PaletteAction::ShowBrainLogs),
        "Brain logs should always be available as a diagnostic view"
    );
}

#[test]
fn logs_view_palette_only_lists_log_actions_and_return() {
    let state = PaletteState::new_logs_view(false);
    assert_eq!(
        action_order(&state),
        vec![
            PaletteAction::ShowReceiverServerStatus,
            PaletteAction::ShowSyncStatus,
            PaletteAction::ShowBrainLogs,
            PaletteAction::ReturnToMainView
        ]
    );
}

#[test]
fn open_links_shown_for_habit_with_notes_url() {
    // A habit has no Linear issue but can carry a notes URL; the command
    // is offered (works_on_habits) and gated only on having ≥ 1 link.
    let with_link = PaletteState::new_task_actions(
        "H1".into(),
        "habit".into(),
        true,
        false,
        false,
        LinkKind::SingleNotes,
    );
    assert!(has_open_links(&with_link));

    let no_link = PaletteState::new_task_actions(
        "H1".into(),
        "habit".into(),
        true,
        false,
        false,
        LinkKind::None,
    );
    assert!(!has_open_links(&no_link));
}

fn action_order(state: &PaletteState) -> Vec<PaletteAction> {
    state.visible().iter().map(|c| c.action).collect()
}

// --- PaletteState: daily-triage alert toggle ---

fn daily_triage_label(state: &PaletteState) -> Option<String> {
    state
        .visible()
        .iter()
        .find(|c| matches!(c.action, PaletteAction::ToggleDailyTriageAlert))
        .map(|c| state.label_for(c))
}

#[test]
fn daily_triage_toggle_is_globally_available() {
    // A long-running TUI needs to flip the alert mid-session, so the toggle is
    // a global command shown regardless of selection.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert!(action_order(&state).contains(&PaletteAction::ToggleDailyTriageAlert));
}

#[test]
fn daily_triage_toggle_reads_disable_when_alert_enabled() {
    // Default state: the alert is enabled, so the command offers to disable it.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert_eq!(
        daily_triage_label(&state).as_deref(),
        Some("Disable daily triage alert")
    );
}

#[test]
fn daily_triage_toggle_reads_enable_when_alert_disabled() {
    // Seeded from `App::skip_daily_triage_check` at open time (like
    // `receiver_server_running`); when disabled the command offers to re-enable.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.daily_triage_alert_disabled = true;
    assert_eq!(
        daily_triage_label(&state).as_deref(),
        Some("Enable daily triage alert")
    );
}

// --- PaletteState: brain-tab switch commands (triage tab) ---

#[test]
fn triage_switch_commands_are_hidden_without_a_triage_tab() {
    // No triage tab open → the palette must not offer to switch to it. This is
    // the `is_visible: if_triage_open` gate.
    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    let actions = action_order(&state);
    assert!(!actions.contains(&PaletteAction::ShowMainBrainSession));
    assert!(!actions.contains(&PaletteAction::ShowDailyTriageSession));
}

#[test]
fn triage_switch_commands_appear_while_a_triage_tab_is_open() {
    // `triage_open` is seeded from `App::triage_brain.is_some()` at open time,
    // like `receiver_server_running`. With it set, both tab-switch rows show —
    // the reliable palette alternative to the terminal-flaky Alt+1 / Alt+2.
    let mut state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    state.triage_open = true;
    let actions = action_order(&state);
    assert!(actions.contains(&PaletteAction::ShowMainBrainSession));
    assert!(actions.contains(&PaletteAction::ShowDailyTriageSession));
}

#[test]
fn full_palette_lists_actions_in_canonical_order() {
    // Task with notes selected: start → complete → message-about →
    // message-global → notes → remove → defer group → other globals.
    let state = PaletteState::new(
        Some("T1".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    assert_eq!(
        action_order(&state),
        vec![
            PaletteAction::StartTask,
            PaletteAction::MarkTaskComplete,
            PaletteAction::MessageBrainAboutTask,
            PaletteAction::SendBrainMessage,
            PaletteAction::StartReceiverServer,
            PaletteAction::ShowReceiverServerStatus,
            PaletteAction::ToggleNotes,
            PaletteAction::RemoveTask,
            PaletteAction::DeferTask(1),
            PaletteAction::DeferTask(7),
            PaletteAction::DeferTask(14),
            PaletteAction::OpenHabitsInBrowser,
            PaletteAction::SyncBrainNow,
            PaletteAction::ShowSyncStatus,
            PaletteAction::OpenAgenda,
            PaletteAction::ShowBrainLogs,
            PaletteAction::ToggleDailyTriageAlert,
            PaletteAction::ReturnToMainView,
        ]
    );
}

#[test]
fn sync_brain_palette_command_has_no_shortcut() {
    use crate::tui::palette::shortcut_for;

    let state = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    let rows = state.numbered_entries();

    assert!(
        rows.iter()
            .any(|(label, shortcut)| label.contains("Sync brain now") && shortcut.is_none()),
        "{rows:?}"
    );
    assert_eq!(shortcut_for(PaletteAction::SyncBrainNow), None);
}

#[test]
fn task_actions_modal_palette_keeps_order_minus_globals() {
    // Same relative order, with the global commands filtered out.
    let state = PaletteState::new_task_actions(
        "T1".into(),
        "task".into(),
        false,
        true,
        false,
        LinkKind::None,
    );
    assert_eq!(
        action_order(&state),
        vec![
            PaletteAction::StartTask,
            PaletteAction::MarkTaskComplete,
            PaletteAction::MessageBrainAboutTask,
            PaletteAction::ToggleNotes,
            PaletteAction::RemoveTask,
            PaletteAction::DeferTask(1),
            PaletteAction::DeferTask(7),
            PaletteAction::DeferTask(14),
        ]
    );
}

// --- PaletteState: numbered rows (brain-menu parity) ---

#[test]
fn palette_rows_are_numbered_from_one_in_canonical_order() {
    // Numbers are the 1-based position in the scope-visible list, stable
    // regardless of the text filter — so the digit a user types always
    // points at the same command.
    let state = PaletteState::new(
        Some("T1".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    let cmds = state.scoped();
    assert_eq!(state.number_for(cmds[0]), 1);
    assert_eq!(state.number_for(cmds[1]), 2);
    assert_eq!(state.number_for(cmds.last().unwrap()), cmds.len());
}

#[test]
fn typing_a_row_number_filters_to_that_numbered_row() {
    // "2." prefixes the second command, so a query of "2" keeps it.
    let mut state = PaletteState::new(
        Some("T1".into()),
        false,
        true,
        false,
        LinkKind::None,
        false,
        false,
    );
    let second = state.scoped()[1];
    state.append('2');
    let hits = state.visible();
    assert!(
        hits.iter().any(|c| c.action == second.action),
        "typing the row number should surface that numbered command"
    );
}
