//! `App` actions: run_* command handlers, daily-triage check,
//! mark-complete, and palette-action dispatch:
//!   - `commands` — the `run_*` / mark-complete / palette-dispatch impl
//!   - `receiver`: persistent receiver intent and palette status
//!   - `triage`   — the daily-triage nudge + rollover logic (and its tests)

mod commands;
mod receiver;
mod triage;
