//! `App` actions: the palette-command dispatcher, the `run_*` handlers, and
//! the daily-triage check:
//!   - `capture_note`   — naming, writing, and opening a `capture/` note
//!   - `commands`       — dispatch + the shared `run_*` / mark-complete impl
//!   - `global`         — running a `GlobalAction`
//!   - `task_commands`  — running a `TaskCommand` against a resolved task
//!   - `entry_commands` — running an `EntryCommand` against a resolved path
//!   - `targets`        — resolving a command's target, or asking for it
//!   - `receiver`       — persistent receiver intent and palette status
//!   - `triage`         — the daily-triage nudge + rollover logic (and tests)

mod capture_note;
mod commands;
mod entry_commands;
mod global;
mod receiver;
mod targets;
mod task_commands;
mod triage;
