//! `App` state: construction, query/filter, selection, notes toggles,
//! and view navigation. The `App` type lives in the `tui` root; this module's
//! submodules each add an `impl App` block by concern:
//!   - `construct`        — `App::new`
//!   - `nav`              — query/filter, body rebuild, scroll, cursor movement
//!   - `view`             — Tab-cycle view switching + CSV reloads
//!   - `selection_query`  — current-entry queries + notes toggles

mod construct;
mod nav;
mod selection_query;
mod view;

pub(crate) use construct::AppInit;
