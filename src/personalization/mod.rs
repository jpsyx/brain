//! Personalization: content-about-you, stored as just another brain config.
//!
//! It lives beside the JSON config store in the brain config dir
//! (`~/.config/brain/personalization.json`; see `settings::config_dir`), which
//! is under `$HOME` — not inside the brain root and not in the jpsyx-configs
//! repo. Syncing that dir across machines is handled externally.
//!
//! It holds identity facts (name, role, who you work for) that skills read via
//! a runtime lookup, plus the user's tag styles that depersonalize the task
//! renderer. Everything is optional and falls back to generic defaults, so the
//! public binary carries no personal taxonomy.

pub mod checklist;
pub mod command;
pub mod model;
pub mod namespaces;
pub mod onboarding;
pub mod runtime;
pub mod store;
pub mod tags;

pub use runtime::{init_tag_styles, tag_label};
