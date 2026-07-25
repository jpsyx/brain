//! Backblaze B2 cross-machine sync (Sub-project C). C1 ships only the parse-only
//! config schema; transport (rclone bisync), the CSV merge, triggers, and skill
//! integration land in C2–C5.

pub mod args;
pub mod command;
pub mod config;
pub mod conflicts;
pub mod journal;
pub mod remote;
pub mod run;
pub mod setup;
pub mod verify;

pub use config::SyncConfig;
