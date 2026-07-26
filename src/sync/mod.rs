//! Backblaze B2 cross-machine sync (Sub-project C). C1 ships only the parse-only
//! config schema; transport (rclone bisync), the CSV merge, triggers, and skill
//! integration land in C2–C5.

pub mod args;
pub mod check;
pub mod command;
pub mod config;
pub mod conflicts;
pub mod csv_merge;
pub mod csv_sync;
pub mod journal;
pub mod lock;
pub mod progress;
pub mod remote;
pub mod run;
pub mod setup;
pub mod verify;
