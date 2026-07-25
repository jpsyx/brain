//! Brain **env**: the machine-local config store at `~/.config/brain/env.json`,
//! managed by `brain env {list|get|set}`. Holds values that would be *wrong* on
//! another machine — `root`, `markdown_to_pdf_path`, and the Backblaze `sync`
//! block — so it is never Backblaze-synced (contrast `crate::settings`, the
//! brain **config** store that rides the brain-dir sync).

mod store;

pub use store::env_path;

pub(crate) use store::load_map;
