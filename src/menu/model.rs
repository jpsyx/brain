//! The palette's data model: every action it can run (`Choice`), the
//! contextual targets that gate the entry-action rows (`Targets`), and the
//! pure builders for the ordered row list (`items`) and each row's direct-key
//! hint (`shortcut_for`).

use crate::state::PanelSide;

use super::labels::{create_pdf_label, delete_label, open_dir_label, open_file_label};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Convert the highlighted markdown file to a colocated PDF. Only offered
    /// as a row when a `.md` file is selected (its label carries the
    /// filename), so it is a *conditional* choice, not part of `STATIC_ITEMS`.
    CreatePdf,
    /// Open the highlighted file (the same action as `Enter` in the picker).
    /// Offered as a row only when a *file* is highlighted (its label carries
    /// the filename), so it's a *conditional* choice, not part of
    /// `STATIC_ITEMS`. There's no file to open on a directory.
    OpenFile,
    /// Open the highlighted entry's directory (the same action as
    /// `Ctrl-Enter` / reveal-in-Finder). A file resolves to its parent
    /// directory, a directory to itself. Offered whenever an entry is
    /// highlighted (its label carries the bucket-relative directory path), so
    /// it's a *conditional* choice, not part of `STATIC_ITEMS`.
    OpenDir,
    /// Move the highlighted entry (file or directory) to the Trash. Offered as
    /// a row whenever something is selected (its label carries the filename),
    /// so it's a *conditional* choice, not part of `STATIC_ITEMS`.
    Delete,
    Msg,
    OpenTasks,
    SearchProjects,
    SearchAreas,
    SearchResources,
    SearchArchive,
    GlobalSearch,
    /// Swap which side the brain panel sits on. Only meaningful in the
    /// persistent two-panel TUI; a no-op for the one-shot picker.
    ToggleLayout,
}

/// The static rows, in display order. The layout-toggle row is appended
/// separately because its label depends on the current panel side.
const STATIC_ITEMS: &[(Choice, &str)] = &[
    (Choice::Msg, "Message brain"),
    (Choice::OpenTasks, "Open tasks"),
    (Choice::SearchProjects, "Search projects"),
    (Choice::SearchAreas, "Search areas"),
    (Choice::SearchResources, "Search resources"),
    (Choice::SearchArchive, "Search archive"),
    (Choice::GlobalSearch, "Global search"),
];

/// The label for the layout-toggle row: it names the direction the panel
/// would move, i.e. the *opposite* of where it sits now.
#[must_use]
pub const fn layout_choice_label(side: PanelSide) -> &'static str {
    match side {
        PanelSide::Right => "Move brain panel to the left",
        PanelSide::Left => "Move brain panel to the right",
    }
}

/// The contextual targets for the highlighted entry.
///
/// These drive the conditional palette rows: each field is the pre-formatted
/// text for that row's label (a filename, or a bucket-relative directory
/// path), or `None` when the row shouldn't appear for the current selection.
/// Named fields (rather than a row of same-typed `Option`s) keep the call
/// site unambiguous.
#[derive(Debug, Default, Clone)]
pub struct Targets {
    /// Highlighted markdown filename → "Create PDF for '…'".
    pub pdf: Option<String>,
    /// Highlighted file's filename → "Open file '…'" (files only).
    pub open_file: Option<String>,
    /// Highlighted entry's bucket-relative directory → "Open directory '…'".
    pub open_dir: Option<String>,
    /// Highlighted entry's filename (any kind) → "Delete '…'".
    pub delete: Option<String>,
}

/// The full ordered row list for a given panel side: the static rows plus
/// the dynamically-labeled layout toggle at the end. `include_msg` controls
/// whether the "Message brain" row is offered — the persistent shell hides
/// it while the brain panel is already open (there's nothing to open), and
/// shows it (to re-open the panel) once it's closed. The one-shot picker
/// always includes it.
pub(super) fn items(side: PanelSide, include_msg: bool, targets: &Targets) -> Vec<(Choice, String)> {
    let mut v: Vec<(Choice, String)> = Vec::new();
    // The contextual entry-action rows lead the list so a common action is
    // the default-selected one on open. "Create PDF" keeps the lead when a
    // markdown file is highlighted; "Open file" / "Open directory" follow.
    if let Some(filename) = &targets.pdf {
        v.push((Choice::CreatePdf, create_pdf_label(filename)));
    }
    if let Some(filename) = &targets.open_file {
        v.push((Choice::OpenFile, open_file_label(filename)));
    }
    if let Some(rel_dir) = &targets.open_dir {
        v.push((Choice::OpenDir, open_dir_label(rel_dir)));
    }
    v.extend(
        STATIC_ITEMS
            .iter()
            .filter(|(c, _)| include_msg || *c != Choice::Msg)
            .map(|(c, l)| (*c, (*l).to_owned())),
    );
    v.push((Choice::ToggleLayout, layout_choice_label(side).to_owned()));
    // "Delete" trails the list, deliberately never the default-selected row:
    // a destructive action should not fire from a stray Enter on palette open.
    if let Some(filename) = &targets.delete {
        v.push((Choice::Delete, delete_label(filename)));
    }
    v
}

/// Direct keystroke that fires a choice without opening the palette,
/// rendered as a dim `[…]` annotation next to the palette row. `None` when a
/// row has no direct shortcut.
#[must_use]
pub const fn shortcut_for(choice: Choice) -> Option<&'static str> {
    match choice {
        Choice::CreatePdf => Some("^G"),
        // "Open file" / "Open directory" reuse the picker's existing keys:
        // plain Enter opens the file, Ctrl-Enter reveals its directory.
        Choice::OpenFile => Some("↵"),
        Choice::OpenDir => Some("^↵"),
        Choice::Delete => Some("^D"),
        Choice::Msg => Some("^M"),
        Choice::OpenTasks => Some("^T"),
        Choice::SearchProjects
        | Choice::SearchAreas
        | Choice::SearchResources
        | Choice::SearchArchive
        | Choice::GlobalSearch
        | Choice::ToggleLayout => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<(Choice, String)> {
        items(PanelSide::Right, true, &Targets::default())
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
}
