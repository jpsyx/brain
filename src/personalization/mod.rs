//! Personalization: content-about-you, stored as just another brain config.
//!
//! It lives beside the JSON config store at
//! `<selected-root>/.config/personalization.json` and therefore travels with
//! that workspace.
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

pub use runtime::load_tag_styles;
