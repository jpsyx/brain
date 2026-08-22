//! Brain's persistent ratatui shell.
//!
//! The shell owns three main views (tasks, brain-directory search, and logs)
//! plus one app-level brain panel. The panel persists while the main view
//! changes and every frontend interaction crosses
//! [`AgentController`](crate::agent::AgentController).
//!
//! Runtime ownership is explicit:
//! - `runtime/builder.rs` acquires startup resources and assembles the app;
//! - `runtime/terminal.rs` owns `/dev/tty`, terminal modes, and restoration;
//! - `runtime/mod.rs` owns process-lifetime resources and orderly shutdown;
//! - `event_loop` handles interaction after calling the runtime's tick and
//!   draw boundaries.
//!
//! Module layout:
//! - This file owns the [`App`] composition root; focused state owners keep
//!   their representations private and expose semantic operations.
//! - `palette` / `modals` — the command-palette and confirm / brain-input /
//!   help modal state + behavior.
//! - `app_state` / `app_actions` / `app_brain` — the `App` impl, split by
//!   concern.
//! - `event_loop`: application event dispatch and overlay key routing.
//! - `handlers` / `keymap` — per-modal key handlers and the pure key-decision
//!   helpers.
//! - `draw` / `draw_palette` / `draw_modals` / `draw_help` — the rendering of
//!   each surface.
//! - `shell` — the [`ShellRunner`] injection boundary.

pub(crate) mod action;
mod app_actions;
mod app_brain;
mod app_skill_session;
mod app_state;
mod app_sync;
mod draw;
mod draw_assignee;
mod draw_help;
mod draw_modals;
mod draw_palette;
mod draw_sync_log;
mod event_loop;
mod handlers;
mod keymap;
mod launch;
pub(crate) mod links;
mod logs_view;
pub(crate) mod modal_state;
mod modals;
mod overlay;
pub(crate) mod palette;
pub mod receiver;
mod runtime;
mod search_view;
mod shell;
pub mod singleton;
mod state;
mod status_warning;

#[cfg(test)]
mod tests;

pub(crate) use event_loop::run_tui;
pub(crate) use launch::TuiLaunch;

use ratatui::style::Color;

use self::overlay::Overlay;
use self::state::{AppContext, AppServices, BrainPanelState, ShellState, StatusState, TasksState};

/// Subtle "elevation" background for the row(s) belonging to the currently
/// selected task.
const SELECTED_BG: Color = Color::Rgb(50, 56, 78);

mod model;

pub(crate) struct App {
    context: AppContext,
    tasks: TasksState,
    brain: BrainPanelState,
    shell: ShellState,
    overlay: Option<Overlay>,
    services: AppServices,
    status: StatusState,
    receiver: crate::tui::receiver::ReceiverRuntime,
}
