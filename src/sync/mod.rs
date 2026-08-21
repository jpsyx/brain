//! Backblaze B2 cross-machine sync (Sub-project C). C1 ships only the parse-only
//! config schema; transport (rclone bisync), the CSV merge, triggers, and skill
//! integration land in C2–C5.

pub mod args;
pub mod check;
pub mod check_access;
pub mod command;
pub mod config;
pub mod conflicts;
pub mod counters;
pub mod csv_merge;
pub mod csv_sync;
pub mod current;
pub mod follow;
pub mod freshness;
pub mod identity;
pub mod journal;
pub mod lock;
pub mod periodic;
pub mod progress;
pub mod remote;
pub mod run;
pub mod setup;
pub mod trigger;
pub mod verify;
pub mod watch;
