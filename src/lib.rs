//! Library surface for `brain`. The binary at `src/main.rs` is the
//! user-facing entry point; this file exists so integration tests in
//! `tests/` can reach the pure modules (entry collection, path resolution,
//! render helpers) without going through argv or a TUI.
//!
//! `picker` and `menu` run a real ratatui frontend against `/dev/tty`, so
//! their *interactive* halves are not driven from tests; their pure logic
//! (matching, grouping, navigation) is covered by `#[cfg(test)]` units in
//! the modules themselves.

pub mod access;
pub mod actor;
pub mod agent;
pub mod cli;
pub mod command;
pub mod config;
pub mod confirm;
pub mod entry;
pub mod env;
pub mod logging;
pub mod main_view;
pub mod menu;
pub mod migration;
pub mod open_target;
pub mod paths;
pub mod personalization;
pub mod picker;
pub mod pty_pane;
pub mod reindex;
pub mod render;
pub mod server;
pub mod session;
pub mod settings;
pub mod skill_session;
pub mod skills;
pub mod startup_migration;
pub mod state;
pub mod sync;
pub mod tasks;
pub mod theme;
pub mod tui;
pub mod users;
pub mod workspace;
