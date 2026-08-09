//! Personalization: one persona per workspace member, stored as just another
//! brain config.
//!
//! It lives beside the JSON config store at
//! `<selected-root>/.config/personalization.json` and therefore travels with
//! that workspace.
//!
//! Each persona holds identity facts (name, role, who that person works for)
//! that skills read via a runtime lookup, plus their tag styles, which
//! depersonalize the task renderer. The store is keyed by portable user ID, so
//! a shared workspace describes each member separately; reads that concern one
//! person default to this machine's local user. Everything is optional and falls
//! back to generic defaults, so the public binary carries no personal taxonomy.

pub mod checklist;
pub mod command;
pub mod namespaces;
pub mod onboarding;
pub mod persona;
pub mod personas;
pub mod runtime;
pub mod store;
pub mod tags;

pub use runtime::load_tag_styles;
