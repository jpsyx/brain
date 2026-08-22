//! Per-surface key handlers (palette / confirm / brain-input / normal /
//! completion / brain / search), split by surface:
//!   - `overlay`    — the captive modal handlers
//!   - `tasks_view` — the tasks main view's normal + search key handlers
//!   - `input`      — mouse-wheel routing + brain-PTY keystroke forwarding

mod input;
mod logs;
mod overlay;
pub(super) mod tasks_view;

pub(crate) use input::{half_page_step, handle_brain_key, handle_mouse, handle_skill_session_key};
pub(crate) use logs::handle_logs_key;
pub(crate) use overlay::{
    handle_assignee_filter_key, handle_brain_input_key, handle_confirm_key, handle_help_key,
    handle_link_picker_key, handle_palette_key, handle_sync_log_key,
};
pub(crate) use tasks_view::{TaskSearchEffect, handle_normal_key, handle_search_key};

#[cfg(test)]
pub(crate) use input::brain_key_starts_turn;
#[cfg(test)]
pub(crate) use overlay::run_confirm_skip;
