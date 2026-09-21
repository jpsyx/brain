//! The single source of truth for every keybinding the shell exposes.
//!
//! Both the compact footer (a curated subset + `Alt+S`) and the full help modal
//! (`Alt+S` → everything, grouped) render from [`ALL`]. The actual key
//! *handling* lives in `tui::handlers` / `tui::keymap`; this table is the
//! human-facing catalogue, so when you add or change a binding, update its row
//! here and the footer + help modal follow automatically.
//!
//! Each row also names the [`Command`]s it runs. That is what keeps brain's
//! shortcut-parity invariant honest: **no shortcut without a command-palette
//! row**. A binding that is pure navigation or modal-internal input (moving the
//! cursor, paging, typing into a filter) carries an empty list and says so in
//! its description; anything that *does* something carries the command the
//! palette runs for it, and a guard test proves the palette lists it.
//!
//! Keep `keys` short — it's what shows in the footer chip. `label` is the
//! one-word footer caption; `desc` is the fuller sentence the help modal uses.
//!
//! This file owns the model; `table` owns the rows, and `tests` owns the two
//! guards.

use crate::tui::palette::Command;

mod table;

pub(crate) use table::ALL;

/// Which surface a shortcut belongs to. Drives the grouping in the help modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Navigation,
    Views,
    TaskActions,
    BrainDirectory,
    Brain,
    Search,
    Global,
}

impl Group {
    /// Section heading shown in the help modal, in display order.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::Views => "Views",
            Self::TaskActions => "Task actions",
            Self::BrainDirectory => "Brain directory",
            Self::Brain => "Brain panel",
            Self::Search => "Search",
            Self::Global => "Global",
        }
    }

    /// Groups in the order the help modal lists them.
    pub(crate) const ORDER: [Self; 7] = [
        Self::Navigation,
        Self::Views,
        Self::TaskActions,
        Self::BrainDirectory,
        Self::Brain,
        Self::Search,
        Self::Global,
    ];
}

/// One keybinding row.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shortcut {
    /// Display form of the key(s), e.g. `"j / k"`, `"^D"`, `"Alt+S"`.
    pub(crate) keys: &'static str,
    /// One-word caption for the compact footer.
    pub(crate) label: &'static str,
    /// Fuller description shown in the help modal.
    pub(crate) desc: &'static str,
    /// Which surface this belongs to.
    pub(crate) group: Group,
    /// Whether the compact footer shows this binding (before the ellipsis).
    pub(crate) in_footer: bool,
    /// The command-palette command(s) this binding runs. Empty **only** for
    /// pure navigation or modal-internal input, which is not a command.
    ///
    /// This is a specification, not runtime data: nothing dispatches through
    /// it, and the guard test beside it is what reads it to prove every acting
    /// key has a palette row.
    #[cfg_attr(not(test), allow(dead_code, reason = "read by the parity guard test"))]
    pub(crate) commands: &'static [Command],
}

/// The curated subset rendered in the compact footer (those flagged
/// `in_footer`), in table order.
pub(crate) fn footer_subset() -> Vec<&'static Shortcut> {
    ALL.iter().filter(|s| s.in_footer).collect()
}

/// Shortcuts belonging to `group`, in table order. Used by the help modal.
pub(crate) fn in_group(group: Group) -> Vec<&'static Shortcut> {
    ALL.iter().filter(|s| s.group == group).collect()
}

#[cfg(test)]
mod tests;
