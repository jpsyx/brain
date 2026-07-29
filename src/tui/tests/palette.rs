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
fn show_logs_is_available_only_when_a_verbose_log_exists() {
    let without_logs = PaletteState::new(None, false, false, false, LinkKind::None, false, false);
    assert!(
        !action_order(&without_logs).contains(&PaletteAction::ShowLogs),
        "Show logs should be hidden when this run has no verbose log file"
    );

    let with_logs = PaletteState::new(None, false, false, false, LinkKind::None, false, true);
    assert!(
        action_order(&with_logs).contains(&PaletteAction::ShowLogs),
        "Show logs should be present when verbose logging is active"
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
            PaletteAction::ToggleNotes,
            PaletteAction::RemoveTask,
            PaletteAction::DeferTask(1),
            PaletteAction::DeferTask(7),
            PaletteAction::DeferTask(14),
            PaletteAction::OpenHabitsInBrowser,
            PaletteAction::SyncBrainNow,
            PaletteAction::OpenAgenda,
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
