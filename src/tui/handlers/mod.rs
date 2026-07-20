//! Per-surface key handlers (palette / confirm / brain-input / normal /
//! completion / brain / search), split by surface:
//!   - `overlay`    — the captive modal handlers
//!   - `tasks_view` — the tasks main view's normal + search key handlers
//!   - `input`      — mouse-wheel routing + brain-PTY keystroke forwarding

mod input;
mod overlay;
mod tasks_view;

pub(crate) use input::*;
pub(crate) use overlay::*;
pub(crate) use tasks_view::*;
