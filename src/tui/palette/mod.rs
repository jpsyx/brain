//! Command-palette commands, the action enum, and `PaletteState` behavior.
//!
//! The `PaletteState` struct itself lives in the crate root; this module owns
//! the command table (`command`) and the state impl (`state`).

mod command;
mod state;

pub(crate) use command::*;
