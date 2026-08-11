//! The documents a new workspace is created with.
//!
//! `AGENTS.md` is how an agent learns what this directory is and how to behave
//! in it, and `README.md` is the same orientation for a person. Both are seeded
//! only when absent: from the moment they exist they are the user's documents,
//! not Brain's, so an edited copy is never replaced. Every supported frontend
//! resolves `AGENTS.md`, which is why the instructions live there rather than in
//! a frontend-specific filename.

use std::path::Path;

use anyhow::{Context, Result};

/// Instructions an agent follows inside a workspace.
pub(crate) const AGENTS: &str = include_str!("../../templates/workspace/AGENTS.md");

/// The same orientation for a person browsing the directory.
pub(crate) const README: &str = include_str!("../../templates/workspace/README.md");

/// Write both documents into a workspace root, leaving any existing copy alone.
pub(crate) fn seed_documents(root: &Path) -> Result<()> {
    for (name, body) in [("AGENTS.md", AGENTS), ("README.md", README)] {
        let path = root.join(name);
        if path.exists() {
            continue;
        }
        std::fs::write(&path, body)
            .with_context(|| format!("seeding {} at {}", name, path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
