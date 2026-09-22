use super::{ALL, Group, footer_subset, in_group};
use crate::tasks::task::AssignmentUiMode;
use crate::tui::action::GlobalAction;
use crate::tui::palette::{Command, PaletteContext, catalog_rows};

/// A workspace with every capability turned on, so the catalog offers the
/// assignment controls a single-member workspace has no use for.
fn full_workspace() -> PaletteContext {
    PaletteContext {
        assignment_mode: AssignmentUiMode {
            show_in_detail: true,
            show_create_control: true,
            show_reassign_control: true,
            show_filter: true,
        },
        ..PaletteContext::default()
    }
}

#[test]
fn every_shortcut_that_runs_something_has_a_command_palette_row() {
    // brain's shortcut-parity invariant: a key that performs an action must
    // also be reachable from the palette. Bindings that only move the cursor,
    // page, or feed a modal carry no command and are exempt by declaration.
    let listed: Vec<Command> = catalog_rows(&full_workspace())
        .into_iter()
        .map(|row| row.action)
        .collect();

    for shortcut in ALL {
        for command in shortcut.commands {
            assert!(
                listed.contains(command),
                "{} runs {command:?}, which the command palette does not list",
                shortcut.keys
            );
        }
    }
}

#[test]
fn only_navigation_and_modal_input_may_declare_no_command() {
    // Guards the escape hatch: the exemption is for keys that move or type,
    // and each one says so in its own description.
    let exempt: Vec<&str> = ALL
        .iter()
        .filter(|shortcut| shortcut.commands.is_empty())
        .map(|shortcut| shortcut.keys)
        .collect();

    assert_eq!(
        exempt,
        [
            "j / k",
            "d / u",
            "PgDn / PgUp",
            "g / G",
            // The tree sub-view's expand / collapse / toggle: it moves the
            // cursor and opens nodes, which is navigation, not a command.
            "→ / ← / Space",
            // Its vim aliases for the same moves, and the jump to the ends of
            // the selected node's own sibling list: cursor movement too.
            "h j k l",
            "H / L",
            "Alt+U / Alt+D",
            "Esc",
            "^P",
        ]
    );
}

#[test]
fn footer_subset_is_nonempty_and_all_flagged() {
    let subset = footer_subset();
    assert!(!subset.is_empty());
    assert!(subset.iter().all(|s| s.in_footer));
}

#[test]
fn every_shortcut_lands_in_exactly_one_ordered_group() {
    // Each row's group is one of the ORDER groups, and the grouped views
    // partition ALL (no row lost, none double-counted).
    let total: usize = Group::ORDER.iter().map(|g| in_group(*g).len()).sum();
    assert_eq!(total, ALL.len());
}

#[test]
fn help_lists_the_brain_close_and_new_conversation_shortcuts() {
    let brain = in_group(Group::Brain);
    assert!(
        brain
            .iter()
            .any(|s| s.keys == "^X" && s.desc.contains("session"))
    );
    assert!(
        brain
            .iter()
            .any(|s| s.keys == "^N" && s.desc.contains("/new"))
    );
    assert!(
        brain
            .iter()
            .any(|s| s.keys == "Alt+[ / Alt+]" && s.desc.contains("brain-panel tab"))
    );
}

#[test]
fn help_lists_the_brain_directory_bindings() {
    // The brain-directory view's keys used to be absent from help entirely.
    let rows = in_group(Group::BrainDirectory);
    let keys: Vec<&str> = rows.iter().map(|s| s.keys).collect();
    assert_eq!(
        keys,
        [
            "↵",
            "^↵",
            "⌥↵",
            "→ / ← / Space",
            "h j k l",
            "H / L",
            ".",
            "^G",
            "^D",
            "^R"
        ]
    );
}

#[test]
fn help_routes_receiver_enablement_and_everything_else_through_the_palette() {
    assert!(
        ALL.iter()
            .any(|s| s.keys == "^P" && s.desc.contains("every command"))
    );
}

#[test]
fn help_is_advertised_as_alt_s_not_a_bare_key() {
    assert!(
        ALL.iter()
            .any(|s| s.keys == "Alt+S" && s.desc.contains("shortcuts"))
    );
    assert!(!ALL.iter().any(|s| s.keys == "?" || s.keys == "Alt+?"));
}

#[test]
fn help_lists_the_main_view_switch_shortcuts() {
    assert!(
        ALL.iter()
            .any(|s| s.keys == "^L / ^H" && s.desc.contains("main view"))
    );
    assert!(
        ALL.iter()
            .any(|s| s.keys == "^T / ^B" && s.desc.contains("main view"))
    );
}

#[test]
fn showing_the_shortcuts_is_itself_a_palette_command() {
    assert!(
        catalog_rows(&full_workspace())
            .into_iter()
            .any(|row| row.action == Command::Global(GlobalAction::ShowShortcuts))
    );
}
