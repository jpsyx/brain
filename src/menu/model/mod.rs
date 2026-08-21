//! The search palette's feature-owned actions (`SearchAction`), the
//! contextual targets that gate the entry-action rows (`Targets`), and the
//! pure builders for the ordered row list (`items`) and each row's direct-key
//! hint (`shortcut_for`).

use crate::state::PanelSide;
use crate::tui::{GlobalAction, PaletteRow};

use super::labels::{create_pdf_label, delete_label, open_dir_label, open_file_label};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SearchAction {
    Global(GlobalAction),
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
    SearchProjects,
    SearchAreas,
    SearchResources,
    SearchArchive,
    GlobalSearch,
}

/// The static rows, in display order. The layout-toggle row is appended
/// separately because its label depends on the current panel side.
const STATIC_ITEMS: &[(SearchAction, &str)] = &[
    (
        SearchAction::Global(GlobalAction::MessageBrain),
        "Message brain",
    ),
    (SearchAction::Global(GlobalAction::ShowTasks), "Open tasks"),
    (SearchAction::SearchProjects, "Search projects"),
    (SearchAction::SearchAreas, "Search areas"),
    (SearchAction::SearchResources, "Search resources"),
    (SearchAction::SearchArchive, "Search archive"),
    (SearchAction::GlobalSearch, "Global search"),
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
    /// Persistent receiver intent when this palette belongs to a live TUI.
    /// `None` omits the action from context-free picker uses.
    pub receiver_enabled: Option<bool>,
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
pub(crate) fn items(
    side: PanelSide,
    include_msg: bool,
    targets: &Targets,
) -> Vec<PaletteRow<SearchAction>> {
    let mut rows = Vec::new();
    // The contextual entry-action rows lead the list so a common action is
    // the default-selected one on open. "Create PDF" keeps the lead when a
    // markdown file is highlighted; "Open file" / "Open directory" follow.
    if let Some(filename) = &targets.pdf {
        push_row(
            &mut rows,
            SearchAction::CreatePdf,
            create_pdf_label(filename),
        );
    }
    if let Some(filename) = &targets.open_file {
        push_row(&mut rows, SearchAction::OpenFile, open_file_label(filename));
    }
    if let Some(rel_dir) = &targets.open_dir {
        push_row(&mut rows, SearchAction::OpenDir, open_dir_label(rel_dir));
    }
    for (action, label) in STATIC_ITEMS.iter().filter(|(action, _)| {
        include_msg || *action != SearchAction::Global(GlobalAction::MessageBrain)
    }) {
        push_row(&mut rows, *action, (*label).to_owned());
    }
    if let Some(enabled) = targets.receiver_enabled {
        push_row(
            &mut rows,
            SearchAction::Global(GlobalAction::ToggleReceiver),
            if enabled {
                "Disable receiver"
            } else {
                "Enable receiver"
            }
            .to_owned(),
        );
    }
    push_row(
        &mut rows,
        SearchAction::Global(GlobalAction::ToggleLayout),
        layout_choice_label(side).to_owned(),
    );
    // "Delete" trails the list, deliberately never the default-selected row:
    // a destructive action should not fire from a stray Enter on palette open.
    if let Some(filename) = &targets.delete {
        push_row(&mut rows, SearchAction::Delete, delete_label(filename));
    }
    rows
}

/// Direct keystroke that fires a choice without opening the palette,
/// rendered as a dim `[…]` annotation next to the palette row. `None` when a
/// row has no direct shortcut.
#[must_use]
pub(crate) const fn shortcut_for(action: SearchAction) -> Option<&'static str> {
    match action {
        SearchAction::Global(action) => action.shortcut(),
        SearchAction::CreatePdf => Some("^G"),
        // "Open file" / "Open directory" reuse the picker's existing keys:
        // plain Enter opens the file, Ctrl-Enter reveals its directory.
        SearchAction::OpenFile => Some("↵"),
        SearchAction::OpenDir => Some("^↵"),
        SearchAction::Delete => Some("^D"),
        SearchAction::SearchProjects
        | SearchAction::SearchAreas
        | SearchAction::SearchResources
        | SearchAction::SearchArchive
        | SearchAction::GlobalSearch => None,
    }
}

fn push_row(rows: &mut Vec<PaletteRow<SearchAction>>, action: SearchAction, label: String) {
    let mut row = PaletteRow::new(label, action, shortcut_for(action));
    row.number = rows.len() + 1;
    rows.push(row);
}

#[cfg(test)]
mod tests;
