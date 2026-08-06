//! Persistent user configuration and the `brain config` command.
//!
//! The store is a JSON object at `<brain-root>/.config/config.json` — inside the
//! brain root, so it travels with the brain (whatever syncs the brain syncs the
//! config too). Typed consumers (`config::Config`) deserialize the fields they
//! care about from the same file; this module owns the raw read/modify/write,
//! the declared-variable schema, and the get/set/list CLI. The brain-root
//! pointer itself is the one thing that can't live here (see `crate::paths`).
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

pub(crate) use markdown_pdf::configured_markdown_to_pdf_ready;
pub use markdown_pdf::{ensure_markdown_to_pdf, markdown_to_pdf_command};
pub use render::{render_list, set_confirmation};
pub use schema::Resolved;
pub use store::config_dir;
pub use vars::{normalize_name, resolve_all, resolve_one, set};

pub(crate) use store::{load_map, load_map_at, save_map_at};
