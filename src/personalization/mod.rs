//! Personalization: content-about-you that travels with your brain.
//!
//! Unlike the machine-local config store (`settings/`, at
//! `~/.config/brain/config.json`), personalization lives in a hidden `.config/`
//! dir *inside the brain root* (`<root>/.config/personalization.json`) so it
//! (a) syncs across machines with the brain dir, (b) stays out of Finder
//! (dot-prefixed), and (c) is skipped by the picker's hidden-file filter.
//!
//! It holds identity facts (name, role, who you work for) that skills read via
//! a runtime lookup, plus the user's tag styles that depersonalize the task
//! renderer. Everything is optional and falls back to generic defaults, so the
//! public binary carries no personal taxonomy.

pub mod command;
pub mod model;
pub mod onboarding;
pub mod runtime;
pub mod store;
pub mod tags;

pub use runtime::{init_tag_styles, tag_label};
