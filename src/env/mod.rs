//! Brain **env**: the machine-local config store at `~/.config/brain/env.json`,
//! managed by `brain env {list|get|set}`.
//!
//! The selected workspace record holds machine-specific values such as
//! `markdown_to_pdf_path`, agent launch commands, and the Backblaze `sync`
//! block. Structural fields such as `root` and `workspace_id` are read-only and
//! never enter its free-form `env` map.

mod migrate;
mod schema;
mod store;
mod vars;

pub use migrate::migrate;
pub(crate) use migrate::{migrate_checked, registry_is_valid_v2, registry_setup_needs_migration};
pub use schema::is_sensitive;
pub use vars::{get, resolve_all, resolve_one, set, set_raw};

pub(crate) use store::load_map;
