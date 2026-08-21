use super::*;

fn rows() -> Vec<PaletteRow<SearchAction>> {
    items(PanelSide::Right, true, &Targets::default())
}

#[test]
fn receiver_toggle_uses_persistent_intent_in_the_search_palette() {
    let enabled = items(
        PanelSide::Right,
        true,
        &Targets {
            receiver_enabled: Some(true),
            ..Targets::default()
        },
    );
    let disabled = items(
        PanelSide::Right,
        true,
        &Targets {
            receiver_enabled: Some(false),
            ..Targets::default()
        },
    );

    assert!(enabled.iter().any(|row| {
        row.action == SearchAction::Global(GlobalAction::ToggleReceiver)
            && row.label == "Disable receiver"
    }));
    assert!(disabled.iter().any(|row| {
        row.action == SearchAction::Global(GlobalAction::ToggleReceiver)
            && row.label == "Enable receiver"
    }));
}

/// A `Targets` with just the PDF field set (the common single-row case).
fn pdf_target(name: &str) -> Targets {
    Targets {
        pdf: Some(name.to_owned()),
        ..Targets::default()
    }
}

#[test]
fn message_brain_is_hidden_when_the_panel_is_open() {
    // include_msg = false → the brain panel is already open, so the
    // "Message brain" row is dropped (you can't re-open what's open).
    let closed = items(PanelSide::Right, true, &Targets::default());
    let open = items(PanelSide::Right, false, &Targets::default());
    assert!(
        closed
            .iter()
            .any(|row| { row.action == SearchAction::Global(GlobalAction::MessageBrain) })
    );
    assert!(
        !open
            .iter()
            .any(|row| { row.action == SearchAction::Global(GlobalAction::MessageBrain) })
    );
    assert_eq!(open.len(), closed.len() - 1);
}

// --- the contextual "Create PDF" row --------------------------------

#[test]
fn create_pdf_row_appears_only_with_a_markdown_target() {
    let without = items(PanelSide::Right, true, &Targets::default());
    let with = items(PanelSide::Right, true, &pdf_target("plan.md"));
    assert!(
        !without
            .iter()
            .any(|row| row.action == SearchAction::CreatePdf)
    );
    assert_eq!(with.len(), without.len() + 1);
    // It leads the list so it's the default-selected action on open.
    assert_eq!(with[0].action, SearchAction::CreatePdf);
    assert_eq!(with[0].label, "Create PDF for 'plan.md'");
}

// --- the contextual "Open file" / "Open directory" rows -------------

#[test]
fn open_file_row_appears_only_with_a_file_target_and_leads() {
    let without = items(PanelSide::Right, true, &Targets::default());
    let with = items(
        PanelSide::Right,
        true,
        &Targets {
            open_file: Some("note.md".to_owned()),
            ..Targets::default()
        },
    );
    assert!(
        !without
            .iter()
            .any(|row| row.action == SearchAction::OpenFile)
    );
    assert_eq!(with.len(), without.len() + 1);
    // No PDF target, so "Open file" leads (the default-selected action).
    assert_eq!(with[0].action, SearchAction::OpenFile);
    assert_eq!(with[0].label, "Open file 'note.md'");
}

#[test]
fn open_dir_row_appears_only_with_a_dir_target_and_leads() {
    let without = items(PanelSide::Right, true, &Targets::default());
    let with = items(
        PanelSide::Right,
        true,
        &Targets {
            open_dir: Some("projects/foo".to_owned()),
            ..Targets::default()
        },
    );
    assert!(
        !without
            .iter()
            .any(|row| row.action == SearchAction::OpenDir)
    );
    assert_eq!(with.len(), without.len() + 1);
    assert_eq!(with[0].action, SearchAction::OpenDir);
    assert_eq!(with[0].label, "Open dir 'projects/foo'");
}

#[test]
fn contextual_rows_order_pdf_then_open_file_then_open_dir() {
    // All three entry-action rows lead the list, in this fixed order.
    let all = items(
        PanelSide::Right,
        true,
        &Targets {
            receiver_enabled: None,
            pdf: Some("plan.md".to_owned()),
            open_file: Some("plan.md".to_owned()),
            open_dir: Some("projects/foo".to_owned()),
            delete: Some("plan.md".to_owned()),
        },
    );
    assert_eq!(all[0].action, SearchAction::CreatePdf);
    assert_eq!(all[1].action, SearchAction::OpenFile);
    assert_eq!(all[2].action, SearchAction::OpenDir);
    // Delete still trails, never leads.
    assert_eq!(all.last().unwrap().action, SearchAction::Delete);
}

#[test]
fn open_file_and_open_dir_carry_the_enter_shortcuts() {
    // They surface the picker's existing keys, not new ones.
    assert_eq!(shortcut_for(SearchAction::OpenFile), Some("↵"));
    assert_eq!(shortcut_for(SearchAction::OpenDir), Some("^↵"));
}

#[test]
fn create_pdf_row_carries_the_ctrl_g_shortcut() {
    assert_eq!(shortcut_for(SearchAction::CreatePdf), Some("^G"));
}

#[test]
fn delete_row_appears_only_with_a_target_and_trails_the_list() {
    let without = items(PanelSide::Right, true, &Targets::default());
    let with = items(
        PanelSide::Right,
        true,
        &Targets {
            delete: Some("old.md".to_owned()),
            ..Targets::default()
        },
    );
    assert!(!without.iter().any(|row| row.action == SearchAction::Delete));
    assert_eq!(with.len(), without.len() + 1);
    // It trails the list so a stray Enter on palette open can't delete.
    assert_eq!(with.last().unwrap().action, SearchAction::Delete);
    assert_eq!(with.last().unwrap().label, "Delete 'old.md'");
    assert_ne!(with[0].action, SearchAction::Delete);
}

#[test]
fn delete_row_carries_the_ctrl_d_shortcut() {
    assert_eq!(shortcut_for(SearchAction::Delete), Some("^D"));
}

#[test]
fn menu_rows_are_in_the_expected_order() {
    let order: Vec<SearchAction> = rows().iter().map(|row| row.action).collect();
    assert_eq!(
        order,
        vec![
            SearchAction::Global(GlobalAction::MessageBrain),
            SearchAction::Global(GlobalAction::ShowTasks),
            SearchAction::SearchProjects,
            SearchAction::SearchAreas,
            SearchAction::SearchResources,
            SearchAction::SearchArchive,
            SearchAction::GlobalSearch,
            SearchAction::Global(GlobalAction::ToggleLayout),
        ]
    );
}

#[test]
fn toggle_layout_is_the_last_row_and_names_the_opposite_side() {
    let r = rows();
    let last = r.last().expect("menu is non-empty");
    assert_eq!(
        last.action,
        SearchAction::Global(GlobalAction::ToggleLayout)
    );
    // Panel on the right → offer to move it left.
    assert_eq!(last.label, "Move brain panel to the left");
    // And vice versa.
    assert_eq!(
        layout_choice_label(PanelSide::Left),
        "Move brain panel to the right"
    );
}

#[test]
fn msg_row_is_labeled_message_brain() {
    let r = rows();
    let msg = r
        .iter()
        .find(|row| row.action == SearchAction::Global(GlobalAction::MessageBrain))
        .expect("Msg row exists");
    assert_eq!(msg.label, "Message brain");
}

#[test]
fn every_choice_appears_exactly_once() {
    // Guards against a SearchAction variant being added without a menu row.
    // CreatePdf is conditional (only with a markdown target), so it's
    // checked separately below; the rest must always appear exactly once.
    let all = [
        SearchAction::Global(GlobalAction::MessageBrain),
        SearchAction::Global(GlobalAction::ShowTasks),
        SearchAction::SearchProjects,
        SearchAction::SearchAreas,
        SearchAction::SearchResources,
        SearchAction::SearchArchive,
        SearchAction::GlobalSearch,
        SearchAction::Global(GlobalAction::ToggleLayout),
    ];
    let r = rows();
    assert_eq!(r.len(), all.len());
    for choice in all {
        let count = r.iter().filter(|row| row.action == choice).count();
        assert_eq!(count, 1, "{choice:?} should appear exactly once");
    }
    // With a markdown target, CreatePdf appears exactly once and every
    // other choice still appears exactly once.
    let with_pdf = items(PanelSide::Right, true, &pdf_target("plan.md"));
    assert_eq!(with_pdf.len(), all.len() + 1);
    for choice in all.iter().chain(std::iter::once(&SearchAction::CreatePdf)) {
        let count = with_pdf.iter().filter(|row| &row.action == choice).count();
        assert_eq!(count, 1, "{choice:?} should appear exactly once");
    }
}

#[test]
fn only_msg_and_tasks_carry_shortcuts() {
    assert_eq!(
        shortcut_for(SearchAction::Global(GlobalAction::MessageBrain)),
        Some("^M")
    );
    assert_eq!(
        shortcut_for(SearchAction::Global(GlobalAction::ShowTasks)),
        Some("^T")
    );
    assert_eq!(shortcut_for(SearchAction::SearchProjects), None);
    assert_eq!(shortcut_for(SearchAction::SearchArchive), None);
    assert_eq!(shortcut_for(SearchAction::GlobalSearch), None);
    assert_eq!(
        shortcut_for(SearchAction::Global(GlobalAction::ToggleLayout)),
        None
    );
}

#[test]
fn shared_catalog_rows_use_one_global_action_identity_and_metadata() {
    use crate::tui::{GlobalAction, TaskAction, TaskPalette};

    let search = items(
        PanelSide::Right,
        true,
        &Targets {
            receiver_enabled: Some(false),
            ..Targets::default()
        },
    );
    let tasks = TaskPalette::new(
        None,
        false,
        false,
        false,
        crate::tui::LinkKind::None,
        false,
        false,
    );
    let task_rows = tasks.rows();
    let cases = [
        (
            GlobalAction::MessageBrain,
            "Message brain",
            "Message brain",
            Some("^M"),
            Some("^M"),
        ),
        (
            GlobalAction::ToggleReceiver,
            "Enable receiver",
            "Enable receiver",
            None,
            None,
        ),
        (
            GlobalAction::ShowTasks,
            "Open tasks",
            "Return to main view",
            Some("^T"),
            None,
        ),
    ];

    for (action, search_label, task_label, search_shortcut, task_shortcut) in cases {
        let search_row = search
            .iter()
            .find(|row| row.action == SearchAction::Global(action))
            .expect("shared action appears in the search catalog");
        let task_row = task_rows
            .iter()
            .find(|row| row.action == TaskAction::Global(action))
            .expect("shared action appears in the task catalog");

        assert_eq!(search_row.label, search_label);
        assert_eq!(task_row.label, task_label);
        assert_eq!(search_row.shortcut, search_shortcut);
        assert_eq!(task_row.shortcut, task_shortcut);
    }
}
