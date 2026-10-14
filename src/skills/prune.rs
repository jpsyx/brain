//! Pure removal selection. The transaction verifies ownership before applying it.

/// Historical generation marker. Only validated version markers permit first adoption.
pub const RENDERED_MARKER: &str = ".brain-rendered";

/// Previously rendered skills the current sync did not produce. Pure.
#[must_use]
pub fn stale_rendered(rendered: &[String], installed: &[String]) -> Vec<String> {
    rendered
        .iter()
        .filter(|name| !installed.iter().any(|kept| kept == *name))
        .cloned()
        .collect()
}
