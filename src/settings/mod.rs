//! Persistent user configuration and the `brain config` command.
//!
//! The store is a JSON object at `~/.config/brain/config.json` (or
//! `$XDG_CONFIG_HOME/brain/config.json`) — machine-local, never shipped with
//! the source. Typed consumers (`config::Config`, `paths`) deserialize the
//! fields they care about from the same file; this module owns the raw
//! read/modify/write, the declared-variable schema, and the get/set/list CLI.
//!
//! It also owns the `markdown-to-pdf` prerequisite: the path is a config
//! variable, auto-discovered on first run (PATH, conventional bin dirs, then
//! the login shell so an autoloaded shell-function wrapper is still found) and
//! persisted. A missing or invalid path is a hard, fail-fast error.
//!
//! Split, as everywhere in this crate, into pure decision helpers (schema
//! resolution, table layout, message wording, shell-output parsing) that are
//! unit-tested, and thin IO shells (`load_map`/`save_map`, discovery probes,
//! the process-exiting gate):
//!   - `store`        — locating and reading/writing the JSON object
//!   - `schema`       — the declared `VARS` + `Resolved`
//!   - `vars`         — normalize / get / set / resolve
//!   - `render`       — the `config list` table + `config set` confirmation
//!   - `markdown_pdf` — the `markdown-to-pdf` discovery/validation/gate

mod markdown_pdf;
mod render;
mod schema;
mod store;
mod vars;

pub use markdown_pdf::{ensure_markdown_to_pdf, markdown_to_pdf_command};
pub use render::{color_enabled, render_list, set_confirmation};
pub use store::store_path;
pub use vars::{normalize_name, resolve_all, resolve_one, set};

pub(crate) use store::load_map;
