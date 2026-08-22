//! The `tasks` shell: an interactive ratatui frontend — alternate-screen
//! setup, event loop, drawing.
//!
//! Two input modes:
//!   - **Normal**: scroll keys (j/k/d/u/Space/b/g/G), `/` enters search
//!     mode, q quits, Esc clears an active filter (or quits if none).
//!   - **Search mode**: typing edits the query and live-filters the body.
//!     Esc cancels the filter; Enter exits search mode but keeps the
//!     filter so the user can scroll the results. Ctrl-modified chords
//!     (other than Ctrl+C/Ctrl+U) fall through to normal-mode handling so
//!     task shortcuts like Ctrl+Enter still fire on the highlighted row
//!     without leaving `/`.
//!
//! Ctrl+M opens (or focuses) the persistent brain panel — an interactive
//! `claude` PTY rendered via `tui-term` that resumes the shell's
//! most-recently-active session. Alt+L focuses it, Alt+H focuses the tasks
//! panel. When the brain panel is focused, key events are forwarded to the
//! PTY's stdin as raw bytes — Alt+H is the reliable way to pop focus back to
//! tasks from there. (We deliberately avoid a Space leader and Alt+arrow
//! chords: both collide with editing inside Claude's input.) Ctrl+X closes
//! the panel and ends its agent session.
//!
//! Module layout:
//! - This file owns the [`App`] shell type (and `Panel`) so every submodule
//!   can reach its fields; `overlay` owns the exclusive modal state enum and
//!   `modal_state` owns its task-view state structs.
//! - `palette` / `modals` — the command-palette and confirm / brain-input /
//!   help modal state + behavior.
//! - `app_state` / `app_actions` / `app_brain` — the `App` impl, split by
//!   concern.
//! - `event_loop` — terminal setup ([`run_tui`]), the event loop, and overlay
//!   key routing.
//! - `handlers` / `keymap` — per-modal key handlers and the pure key-decision
//!   helpers.
//! - `draw` / `draw_palette` / `draw_modals` / `draw_help` — the rendering of
//!   each surface.
//! - `shell` — the [`ShellRunner`] injection boundary.

mod action;
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
mod links;
mod logs_view;
mod modal_state;
mod modals;
mod overlay;
mod palette;
pub mod receiver;
mod receiver_state;
mod runtime;
mod search_view;
mod shell;
pub mod singleton;
mod state;
mod status_warning;

#[cfg(test)]
mod tests;

pub(crate) use crate::state::PanelSide;
pub(crate) use event_loop::run_tui;
pub(crate) use launch::TuiLaunch;

// Re-export every submodule's items into the `tui` root so each submodule's
// `use super::*;` can reach its siblings' free functions and shared types
// (the `App` impl is split across files; the handlers / draw / keymap fns
// call across module boundaries). `event_loop` can't be glob-imported because
// its `event_loop` fn would shadow the module name.
pub(crate) use action::*;
pub(crate) use app_state::AppInit;
pub(crate) use app_sync::*;
pub(crate) use draw::*;
pub(crate) use draw_assignee::*;
pub(crate) use draw_help::*;
pub(crate) use draw_modals::*;
pub(crate) use draw_palette::*;
pub(crate) use draw_sync_log::*;
pub(crate) use handlers::*;
pub(crate) use keymap::*;
pub(crate) use links::*;
pub(crate) use logs_view::*;
pub(crate) use modal_state::*;
pub(crate) use overlay::*;
pub(crate) use palette::*;
pub(crate) use search_view::*;
pub(crate) use shell::*;
pub(crate) use state::*;
pub(crate) use status_warning::*;

use ratatui::style::Color;

/// Subtle "elevation" background for the row(s) belonging to the currently
/// selected task.
const SELECTED_BG: Color = Color::Rgb(50, 56, 78);

mod model;

pub(crate) use model::*;

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
