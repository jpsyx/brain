//! The command palette: one vocabulary, one catalog, one surface.
//!
//! `brain` has a single global command palette (`Ctrl+P`) that lists **every**
//! command from every main view. What changes with context is not the list but
//! each row's wording and what running it does first: a command whose target is
//! already highlighted acts on it, and one whose target is missing raises a
//! picker for it.
//!
//! Layout:
//!   - `command`  — the `Command` vocabulary, the ordered catalog, the wording
//!   - `context`  — what the palette knows when it opens, and target resolution
//!   - `model`    — the generic filterable palette widget state
//!   - `sessions` — the per-session data rows the catalog splices in
//!   - `state`    — the global palette and the task actions modal
//!   - `target`   — the "which task?" / "which entry?" pickers

mod command;
mod context;
mod model;
mod sessions;
mod state;
mod target;

#[cfg(test)]
mod model_tests;

pub(crate) use command::{Command, EntryCommand, TaskCommand};
pub(crate) use context::{EntryContext, PaletteContext, TaskContext};
pub(crate) use model::PaletteStep;
pub(crate) use state::CommandPaletteState;
pub(crate) use target::{EntryPickerStep, EntryTargetPicker, TaskChoice, TaskTargetPicker};

#[cfg(test)]
pub(crate) use command::{catalog_rows, shortcut_for};
#[cfg(test)]
pub(crate) use model::{CommandPalette, PaletteControls, PaletteRow};
