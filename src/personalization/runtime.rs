//! Process-wide, read-only tag styles for the renderer.
//!
//! The renderer (`tasks/render`) resolves a tag to its display label on a hot
//! path and in many places, so rather than thread `&TagStyles` through every
//! signature we load the user's styles once into a process cell at startup.
//! The pure resolution logic lives in [`super::tags`] and is unit-tested there;
//! this module is only the thin data supply.
//!
//! Until [`init_tag_styles`] runs, [`tag_label`] falls back to the generic
//! defaults, which keeps unit tests hermetic (they never see the dev machine's
//! personalization).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::tags::TagStyles;

static TAG_STYLES: OnceLock<TagStyles> = OnceLock::new();

/// Load the user's tag styles from the store into the process cell (once).
/// Safe to call from any entry path; only the first call wins.
pub fn init_tag_styles() {
    let _ = TAG_STYLES.set(TagStyles::with_overrides(&super::store::load().tag_styles));
}

/// Resolve a tag's display label using the process tag styles, or the generic
/// defaults if [`init_tag_styles`] has not run.
#[must_use]
pub fn tag_label(tag: &str) -> String {
    TAG_STYLES.get().map_or_else(
        || TagStyles::with_overrides(&BTreeMap::new()).label(tag),
        |s| s.label(tag),
    )
}
