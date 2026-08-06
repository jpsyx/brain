use super::*;

fn rows() -> Vec<(Choice, String)> {
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

    assert!(enabled.contains(&(Choice::ToggleReceiver, "Disable receiver".to_owned())));
    assert!(disabled.contains(&(Choice::ToggleReceiver, "Enable receiver".to_owned())));
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
    assert!(closed.iter().any(|(c, _)| *c == Choice::Msg));
    assert!(!open.iter().any(|(c, _)| *c == Choice::Msg));
    assert_eq!(open.len(), closed.len() - 1);
}

// --- the contextual "Create PDF" row --------------------------------

#[test]
fn create_pdf_row_appears_only_with_a_markdown_target() {
    let without = items(PanelSide::Right, true, &Targets::default());
    let with = items(PanelSide::Right, true, &pdf_target("plan.md"));
    assert!(!without.iter().any(|(c, _)| *c == Choice::CreatePdf));
    assert_eq!(with.len(), without.len() + 1);
    // It leads the list so it's the default-selected action on open.
    assert_eq!(with[0].0, Choice::CreatePdf);
    assert_eq!(with[0].1, "Create PDF for 'plan.md'");
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
    assert!(!without.iter().any(|(c, _)| *c == Choice::OpenFile));
    assert_eq!(with.len(), without.len() + 1);
    // No PDF target, so "Open file" leads (the default-selected action).
    assert_eq!(with[0].0, Choice::OpenFile);
    assert_eq!(with[0].1, "Open file 'note.md'");
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
    assert!(!without.iter().any(|(c, _)| *c == Choice::OpenDir));
    assert_eq!(with.len(), without.len() + 1);
    assert_eq!(with[0].0, Choice::OpenDir);
    assert_eq!(with[0].1, "Open dir 'projects/foo'");
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
    assert_eq!(all[0].0, Choice::CreatePdf);
    assert_eq!(all[1].0, Choice::OpenFile);
    assert_eq!(all[2].0, Choice::OpenDir);
    // Delete still trails, never leads.
    assert_eq!(all.last().unwrap().0, Choice::Delete);
}

#[test]
fn open_file_and_open_dir_carry_the_enter_shortcuts() {
    // They surface the picker's existing keys, not new ones.
    assert_eq!(shortcut_for(Choice::OpenFile), Some("↵"));
    assert_eq!(shortcut_for(Choice::OpenDir), Some("^↵"));
}

#[test]
fn create_pdf_row_carries_the_ctrl_g_shortcut() {
    assert_eq!(shortcut_for(Choice::CreatePdf), Some("^G"));
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
    assert!(!without.iter().any(|(c, _)| *c == Choice::Delete));
    assert_eq!(with.len(), without.len() + 1);
    // It trails the list so a stray Enter on palette open can't delete.
    assert_eq!(with.last().unwrap().0, Choice::Delete);
    assert_eq!(with.last().unwrap().1, "Delete 'old.md'");
    assert_ne!(with[0].0, Choice::Delete);
}

#[test]
fn delete_row_carries_the_ctrl_d_shortcut() {
    assert_eq!(shortcut_for(Choice::Delete), Some("^D"));
}

#[test]
fn menu_rows_are_in_the_expected_order() {
    let order: Vec<Choice> = rows().iter().map(|(c, _)| *c).collect();
    assert_eq!(
        order,
        vec![
            Choice::Msg,
            Choice::OpenTasks,
            Choice::SearchProjects,
            Choice::SearchAreas,
            Choice::SearchResources,
            Choice::SearchArchive,
            Choice::GlobalSearch,
            Choice::ToggleLayout,
        ]
    );
}

#[test]
fn toggle_layout_is_the_last_row_and_names_the_opposite_side() {
    let r = rows();
    let last = r.last().expect("menu is non-empty");
    assert_eq!(last.0, Choice::ToggleLayout);
    // Panel on the right → offer to move it left.
    assert_eq!(last.1, "Move brain panel to the left");
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
        .find(|(c, _)| *c == Choice::Msg)
        .expect("Msg row exists");
    assert_eq!(msg.1, "Message brain");
}

#[test]
fn every_choice_appears_exactly_once() {
    // Guards against a Choice variant being added without a menu row.
    // CreatePdf is conditional (only with a markdown target), so it's
    // checked separately below; the rest must always appear exactly once.
    let all = [
        Choice::Msg,
        Choice::OpenTasks,
        Choice::SearchProjects,
        Choice::SearchAreas,
        Choice::SearchResources,
        Choice::SearchArchive,
        Choice::GlobalSearch,
        Choice::ToggleLayout,
    ];
    let r = rows();
    assert_eq!(r.len(), all.len());
    for choice in all {
        let count = r.iter().filter(|(c, _)| *c == choice).count();
        assert_eq!(count, 1, "{choice:?} should appear exactly once");
    }
    // With a markdown target, CreatePdf appears exactly once and every
    // other choice still appears exactly once.
    let with_pdf = items(PanelSide::Right, true, &pdf_target("plan.md"));
    assert_eq!(with_pdf.len(), all.len() + 1);
    for choice in all.iter().chain(std::iter::once(&Choice::CreatePdf)) {
        let count = with_pdf.iter().filter(|(c, _)| c == choice).count();
        assert_eq!(count, 1, "{choice:?} should appear exactly once");
    }
}

#[test]
fn only_msg_and_tasks_carry_shortcuts() {
    assert_eq!(shortcut_for(Choice::Msg), Some("^M"));
    assert_eq!(shortcut_for(Choice::OpenTasks), Some("^T"));
    assert_eq!(shortcut_for(Choice::SearchProjects), None);
    assert_eq!(shortcut_for(Choice::SearchArchive), None);
    assert_eq!(shortcut_for(Choice::GlobalSearch), None);
    assert_eq!(shortcut_for(Choice::ToggleLayout), None);
}
