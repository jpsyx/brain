//! Terminal setup (`run_tui`), the event loop, and modal key routing:
//!   - `setup`       — `run_tui` terminal enter/teardown + `App` construction
//!   - `run`         — the polling event loop and keystroke dispatch
//!   - `modal_route` — routing from the shell's single overlay enum

mod modal_route;
mod run;
mod setup;

pub(super) use run::event_loop;
pub(crate) use setup::run_tui;
