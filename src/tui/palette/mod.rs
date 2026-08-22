//! Command-palette commands, the action enum, and `TaskPalette` behavior.
//!
//! The `TaskPalette` struct itself lives in the crate root; this module owns
//! the command table (`command`) and the state impl (`state`).

mod command;
mod model;
mod state;

#[cfg(test)]
mod model_tests;

pub(crate) use command::TaskAction;
pub(crate) use model::{CommandPalette, PaletteControls, PaletteRow, PaletteStep};

#[cfg(test)]
pub(crate) use command::shortcut_for;
