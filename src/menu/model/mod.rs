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
    /// Persistently invert receiver intent for the selected workspace.
    ToggleReceiver,
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
pub(super) fn items(
    side: PanelSide,
    include_msg: bool,
    targets: &Targets,
) -> Vec<(Choice, String)> {
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
    if let Some(enabled) = targets.receiver_enabled {
        v.push((
            Choice::ToggleReceiver,
            if enabled {
                "Disable receiver"
            } else {
                "Enable receiver"
            }
            .to_owned(),
        ));
    }
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
        | Choice::ToggleLayout
        | Choice::ToggleReceiver => None,
    }
}

#[cfg(test)]
mod tests;
