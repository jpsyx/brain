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
//! - This file owns the [`App`] composition root; focused state owners keep
//!   their representations private and expose semantic operations.
//! - `palette` / `modals` — the command-palette and confirm / brain-input /
//!   help modal state + behavior.
//! - `app_state` / `app_actions` / `app_brain` — the `App` impl, split by
//!   concern.
//! - `event_loop`: terminal setup ([`run_tui`]), the event loop, and overlay
//!   key routing.
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
