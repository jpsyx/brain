//! Rendering a bundled skill into the files to install.
//!
//! B1 is a byte passthrough — nothing personal is injected yet. Sub-project B2
//! adds extension injection here (base skill + the user's
//! `~/brain/.config/extensions/<name>.md`), always producing a *new built copy*
//! and never mutating the embedded source.

use std::path::PathBuf;

use super::embed::BundledSkill;

/// A file to write into the built skill dir.
pub struct RenderedFile {
    pub rel_path: PathBuf,
    pub contents: Vec<u8>,
}

/// Render a bundled skill to its installable files. B1: identity passthrough.
#[must_use]
pub fn render(skill: &BundledSkill) -> Vec<RenderedFile> {
    skill
        .files
        .iter()
        .map(|f| RenderedFile {
            rel_path: f.rel_path.clone(),
            contents: f.contents.clone(),
        })
        .collect()
}
