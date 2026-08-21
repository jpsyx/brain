//! `App` construction and task-store coordination. Pure task-list state lives
//! in `tui::state::TasksState`; this module retains only the IO boundary that
//! reloads CSV rows into that aggregate.

mod construct;
mod view;

pub(crate) use construct::AppInit;
