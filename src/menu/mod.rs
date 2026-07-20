//! The top-level command palette: the list of every action `brain` can run.
//!
//! It is **not** a standalone screen. The picker / two-panel TUI opens it
//! with `Ctrl-p` and renders it as a modal overlay (`draw_modal`) on top of
//! the current search, driving it through this module's pure `MenuApp` +
//! `handle_key`. Esc closes the overlay; Enter confirms a `Choice` the host
//! acts on. The rows: per-bucket pickers, global search, go-to-root, the
//! claude-msg path, open tasks, and the layout-swap toggle.
//!
//! The palette doubles as a text input: typing filters the rows. Each row's
//! matchable text includes its 1-based number (`"1. Message brain"`), so the
//! user can type the digit *or* any word from the label and the list
//! narrows to the hits. `↑`/`↓`, `Ctrl-k`/`Ctrl-j`, and `Ctrl-p`/`Ctrl-n`
//! cycle the (filtered) list.
//!
//! One row carries a **dynamic** label: the layout toggle reads "Move brain
//! panel to the left" or "...right" depending on where the panel currently
//! sits, so the palette is built per-open with the current `PanelSide`.
//!
//! Layout:
//!   - `labels` — pure elision for the contextual (filename/dir) row labels
//!   - `model`  — `Choice`, `Targets`, the ordered row list, `shortcut_for`
//!   - `filter` — the substring/number matcher over rows
//!   - `app`    — `MenuApp` state + the pure `handle_key`
//!   - `view`   — `draw_modal` and its pure sizing/line builders

mod app;
mod filter;
mod labels;
mod model;
mod view;

pub use app::{MenuApp, Step, handle_key};
pub use model::{Choice, Targets};
pub use view::draw_modal;
