//! Library surface for `brain`. The binary at `src/main.rs` is the
//! user-facing entry point; this file exists so integration tests in
//! `tests/` can reach the pure modules (entry collection, path resolution,
//! render helpers) without going through argv or a TUI.
//!
//! `picker` and `menu` run a real ratatui frontend against `/dev/tty`, so
//! their *interactive* halves are not driven from tests; their pure logic
//! (matching, grouping, navigation) is covered by `#[cfg(test)]` units in
//! the modules themselves.

pub mod cli;
pub mod config;
pub mod confirm;
pub mod entry;
pub mod main_view;
pub mod menu;
pub mod open_target;
pub mod paths;
pub mod personalization;
pub mod picker;
pub mod pty_pane;
pub mod render;
pub mod session;
pub mod settings;
pub mod skills;
pub mod state;
pub mod tasks;
pub mod tui;
