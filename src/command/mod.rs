//! Focused command handlers and top-level dispatch.

pub mod backlog;
pub mod configuration;
pub mod dispatch;
pub mod reindex;
pub mod server;
pub mod sync;
pub mod tasks;
pub mod triage;
pub mod users;
pub mod workspace;

pub(crate) use configuration::prompt_tty_line;
