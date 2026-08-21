//! Terminal setup (`run_tui`), the event loop, and modal key routing:
//!   - `setup`       — `run_tui` terminal enter/teardown + `App` construction
//!   - `run`         — the polling event loop and keystroke dispatch
//!   - `modal_route` — the pure modal-precedence routing

mod modal_route;
mod run;
mod setup;

pub(crate) use setup::run_tui;

// The modal-routing types are referenced within `run` directly; the only
// out-of-module consumer is the unit-test module, so the re-export is
// test-only.
#[cfg(test)]
pub(crate) use modal_route::{ActiveModals, ModalInput, modal_input_target};
