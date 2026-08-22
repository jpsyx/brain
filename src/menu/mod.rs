//! The brain-search command palette catalog and renderer.
//!
//! It is **not** a standalone screen. The picker / two-panel TUI opens it
//! with `Ctrl-p` and renders it as a modal overlay (`draw_modal`) on top of
//! the current search. The shell drives the shared
//! [`crate::tui::CommandPalette`] state with feature-owned [`SearchAction`]
//! values. Esc closes the overlay; Enter confirms an action the host runs.
//!
//! The palette doubles as a text input: typing filters the rows. Each row's
//! matchable text includes its 1-based number (`"1. Message brain"`), so the
//! user can type the digit *or* any word from the label and the list
//! narrows to the hits. `↑`/`↓`, `Ctrl-k`/`Ctrl-j`, and `Ctrl-p`/`Ctrl-n`
//! move through the filtered list.
//!
//! One row carries a **dynamic** label: the layout toggle reads "Move brain
//! panel to the left" or "...right" depending on where the panel currently
//! sits, so the palette is built per-open with the current `PanelSide`.
//!
//! Layout:
//!   - `labels` — pure elision for the contextual (filename/dir) row labels
//!   - `model`: `SearchAction`, `Targets`, the ordered row list,
//!     `shortcut_for`
//!   - `view`   — `draw_modal` and its pure sizing/line builders

mod labels;
mod model;
mod view;

pub(crate) use model::{SearchAction, Targets, items};
pub(crate) use view::draw_modal;

pub(crate) type SearchPalette = crate::tui::palette::CommandPalette<SearchAction>;
