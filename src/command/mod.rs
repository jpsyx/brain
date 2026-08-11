//! Focused command handlers and top-level dispatch.

pub mod configuration;
pub mod dispatch;
pub mod reindex;
pub mod server;
pub mod sync;
pub mod tasks;
pub mod users;
pub mod workspace;

pub(crate) use configuration::prompt_tty_line;
